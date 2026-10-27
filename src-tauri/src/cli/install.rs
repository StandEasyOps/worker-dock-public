use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use sha2::Digest;
use tar::Archive;
use tokio::io::AsyncWriteExt;
use zip::ZipArchive;

use crate::cli::error::CliError;
use crate::cli::paths::{
    HostOs, binary_name_in_version_dir, cache_dir, host_arch, host_os, linux_shim_dir,
    official_archive_url, official_version_dir, official_versions_dir, windows_shim_dir,
};
use crate::cli::version::{expected_archive_sha256, is_cli_version, is_vetted};
use crate::privacy;

const DOWNLOAD_TIMEOUT_SECS: u64 = 600;
const SCRIPT_TIMEOUT_SECS: u64 = 20;
const SCRIPT_SIZE_LIMIT: usize = 1024 * 1024;
const ARCHIVE_SIZE_LIMIT: u64 = 400 * 1024 * 1024;
const EXTRACT_SIZE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Progress {
    pub phase: String,
    pub detail: String,
}

pub fn metadata_client() -> Result<reqwest::Client, CliError> {
    Ok(reqwest::Client::builder()
        .user_agent(crate::app_info::user_agent())
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(4))
        .timeout(std::time::Duration::from_secs(SCRIPT_TIMEOUT_SECS))
        .build()?)
}

pub fn download_client() -> Result<reqwest::Client, CliError> {
    Ok(reqwest::Client::builder()
        .user_agent(crate::app_info::user_agent())
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(4))
        .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .build()?)
}

pub async fn fetch_official_latest() -> Result<Option<String>, CliError> {
    let url = crate::cli::paths::official_install_script_url(host_os())?;
    let response = metadata_client()?.get(url).send().await?;
    if !response.status().is_success() {
        return Err(CliError::msg(format!(
            "official install script returned HTTP {}",
            response.status()
        )));
    }
    let text = response.text().await?;
    if text.len() > SCRIPT_SIZE_LIMIT {
        return Err(CliError::msg(
            "official install script is unexpectedly large",
        ));
    }
    Ok(crate::cli::version::version_from_installer_text(&text))
}

pub async fn download_archive(
    version: &str,
    mut on_progress: impl FnMut(Progress),
) -> Result<PathBuf, CliError> {
    if !is_cli_version(version) {
        return Err(CliError::msg("refusing to download an invalid CLI version"));
    }
    if !is_vetted(version) {
        return Err(CliError::msg(format!(
            "{version} is not a vetted official CLI version"
        )));
    }
    let url = official_archive_url(version, host_os(), host_arch()?)?;
    on_progress(Progress {
        phase: "downloading".into(),
        detail: url.to_string(),
    });
    let cache = cache_dir()?;
    privacy::ensure_private_dir(&cache).map_err(CliError::from)?;
    let filename = url
        .path_segments()
        .and_then(std::iter::Iterator::last)
        .ok_or_else(|| CliError::msg("archive URL is missing a filename"))?;
    let dest = cache.join(format!("{version}-{filename}"));
    let response = download_client()?.get(url.clone()).send().await?;
    if !response.status().is_success() {
        return Err(CliError::msg(format!(
            "download failed: HTTP {} from {url}",
            response.status()
        )));
    }
    if let Some(length) = response.content_length() {
        if length > ARCHIVE_SIZE_LIMIT {
            return Err(CliError::msg(format!(
                "official archive is {length} bytes; refusing a download over {ARCHIVE_SIZE_LIMIT}"
            )));
        }
    }
    let mut file = tokio::fs::File::create(&dest).await?;
    let mut stream = response;
    let mut hasher = sha2::Sha256::new();
    let mut written = 0_u64;
    while let Some(chunk) = stream.chunk().await? {
        written = written.saturating_add(chunk.len() as u64);
        if written > ARCHIVE_SIZE_LIMIT {
            drop(file);
            let _ = fs::remove_file(&dest);
            return Err(CliError::msg(
                "official archive exceeded the download size limit; refusing to install",
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    privacy::restrict_path(&dest).map_err(CliError::from)?;
    let digest = hex_digest(hasher.finalize().as_slice());
    if let Err(error) = verify_archive_digest(version, &digest) {
        let _ = fs::remove_file(&dest);
        return Err(error);
    }
    Ok(dest)
}

pub fn verify_archive_digest(version: &str, digest: &str) -> Result<(), CliError> {
    let os = match host_os() {
        HostOs::Linux => "linux",
        HostOs::Windows => "windows",
    };
    let arch = host_arch()?;
    let expected = expected_archive_sha256(version, os, arch).ok_or_else(|| {
        CliError::msg(format!(
            "no pinned SHA-256 for official CLI {version}/{os}/{arch}"
        ))
    })?;
    if digest.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(CliError::msg(format!(
            "official CLI archive SHA-256 did not match the pin for {version}/{os}/{arch}; refusing to install"
        )))
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn extract_archive(archive: &Path, version: &str) -> Result<PathBuf, CliError> {
    if !is_cli_version(version) {
        return Err(CliError::msg("refusing to extract an invalid CLI version"));
    }
    let versions = official_versions_dir()?;
    privacy::ensure_private_dir(&versions).map_err(CliError::from)?;
    let staging = versions.join(format!(".tmp-{version}-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let extract_result = if archive.extension().and_then(|ext| ext.to_str()) == Some("zip") {
        extract_zip(archive, &staging, 1)
    } else {
        extract_targz(archive, &staging, 1)
    };
    if let Err(error) = extract_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let final_dir = official_version_dir(version)?;
    if final_dir.exists() {
        fs::remove_dir_all(&final_dir)?;
    }
    fs::rename(&staging, &final_dir)?;
    let binary = final_dir.join(binary_name_in_version_dir(host_os()));
    if !binary.is_file() {
        return Err(CliError::msg(format!(
            "archive did not contain {}",
            binary.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&binary)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions)?;
    }
    Ok(final_dir)
}

pub fn extract_targz(archive: &Path, dest: &Path, strip_components: usize) -> Result<(), CliError> {
    let file = File::open(archive)?;
    let decoder = GzDecoder::new(file);
    let mut tarball = Archive::new(decoder);
    let mut budget = EXTRACT_SIZE_LIMIT;
    for entry in tarball.entries()? {
        let mut entry = entry.map_err(|error| CliError::msg(format!("tar entry: {error}")))?;
        let raw = entry
            .path()
            .map_err(|error| CliError::msg(format!("tar path: {error}")))?
            .into_owned();
        let Some(relative) = stripped_relative(&raw, strip_components)? else {
            continue;
        };
        let out = dest.join(&relative);
        write_safe_file(
            &out,
            dest,
            entry.header().entry_type().is_dir(),
            entry.size(),
            &mut budget,
            &mut entry,
        )?;
    }
    Ok(())
}

pub fn extract_zip(archive: &Path, dest: &Path, strip_components: usize) -> Result<(), CliError> {
    let file = File::open(archive)?;
    let mut zip = ZipArchive::new(file).map_err(|error| CliError::msg(format!("zip: {error}")))?;
    let mut budget = EXTRACT_SIZE_LIMIT;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| CliError::msg(format!("zip entry: {error}")))?;
        let raw = PathBuf::from(
            entry
                .name()
                .map_err(|error| CliError::msg(format!("zip name: {error}")))?
                .as_ref(),
        );
        let Some(relative) = stripped_relative(&raw, strip_components)? else {
            continue;
        };
        let out = dest.join(&relative);
        write_safe_file(
            &out,
            dest,
            entry.is_dir(),
            entry.size(),
            &mut budget,
            &mut entry,
        )?;
    }
    Ok(())
}

fn stripped_relative(raw: &Path, strip_components: usize) -> Result<Option<PathBuf>, CliError> {
    let mut parts = Vec::new();
    for component in raw.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CliError::msg(format!(
                    "refusing archive path with traversal: {}",
                    raw.display()
                )));
            }
        }
    }
    if parts.len() <= strip_components {
        return Ok(None);
    }
    let mut relative = PathBuf::new();
    for part in parts.into_iter().skip(strip_components) {
        relative.push(part);
    }
    Ok(Some(relative))
}

fn write_safe_file(
    out: &Path,
    dest: &Path,
    is_dir: bool,
    declared_size: u64,
    budget: &mut u64,
    reader: &mut impl Read,
) -> Result<(), CliError> {
    let dest_canon = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    if is_dir {
        fs::create_dir_all(out)?;
        return Ok(());
    }
    if declared_size > ARCHIVE_SIZE_LIMIT {
        return Err(CliError::msg(format!(
            "refusing archive member larger than {ARCHIVE_SIZE_LIMIT} bytes"
        )));
    }
    if declared_size > *budget {
        return Err(CliError::msg(
            "archive extract exceeded the uncompressed size limit",
        ));
    }
    *budget = budget.saturating_sub(declared_size);
    if let Some(parent) = out.parent() {
        let parent_canon = parent.canonicalize()?;
        if !parent_canon.starts_with(&dest_canon) {
            return Err(CliError::msg(format!(
                "refusing to extract outside target: {}",
                out.display()
            )));
        }
    }
    let mut file = File::create(out)?;
    io::copy(reader, &mut file)?;
    file.flush()?;
    Ok(())
}

pub fn link_shims(version_dir: &Path) -> Result<PathBuf, CliError> {
    let os = host_os();
    let binary = version_dir.join(binary_name_in_version_dir(os));
    if !binary.is_file() {
        return Err(CliError::msg(format!(
            "missing official binary {}",
            binary.display()
        )));
    }
    match os {
        HostOs::Linux => {
            let bin = linux_shim_dir()?;
            fs::create_dir_all(&bin)?;
            replace_symlink(&bin.join("agent"), &binary)?;
            replace_symlink(&bin.join("cursor-agent"), &binary)?;
            Ok(bin.join("agent"))
        }
        HostOs::Windows => {
            let shims = windows_shim_dir()?;
            fs::create_dir_all(&shims)?;
            for name in ["cursor-agent.exe", "cursor-agent.cmd", "cursor-agent.ps1"] {
                let source = version_dir.join(name);
                if source.is_file() {
                    fs::copy(&source, shims.join(name))?;
                }
            }
            if shims.join("cursor-agent.exe").is_file() {
                fs::copy(shims.join("cursor-agent.exe"), shims.join("agent.exe"))?;
            }
            if shims.join("cursor-agent.cmd").is_file() {
                fs::copy(shims.join("cursor-agent.cmd"), shims.join("agent.cmd"))?;
            }
            if shims.join("cursor-agent.ps1").is_file() {
                fs::copy(shims.join("cursor-agent.ps1"), shims.join("agent.ps1"))?;
            }
            let agent = shims.join("agent.exe");
            if agent.is_file() {
                Ok(agent)
            } else {
                Ok(binary)
            }
        }
    }
}

#[cfg(unix)]
fn replace_symlink(link: &Path, target: &Path) -> Result<(), CliError> {
    match fs::remove_file(link) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(not(unix))]
fn replace_symlink(_link: &Path, _target: &Path) -> Result<(), CliError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use tar::Builder;
    use tempfile::tempdir;

    fn sample_targz(dir: &Path) -> PathBuf {
        let archive = dir.join("pkg.tar.gz");
        let file = File::create(&archive).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(2);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "dist/cursor-agent", &b"ok"[..])
            .unwrap();
        builder.finish().unwrap();
        archive
    }

    #[test]
    fn extracts_stripped_targz() {
        let dir = tempdir().unwrap();
        let archive = sample_targz(dir.path());
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();
        extract_targz(&archive, &dest, 1).unwrap();
        assert_eq!(fs::read(dest.join("cursor-agent")).unwrap(), b"ok");
    }

    #[test]
    fn rejects_zip_traversal() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("evil.zip");
        let file = File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("../escape", options).unwrap();
        zip.write_all(b"nope").unwrap();
        zip.finish().unwrap();
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();
        assert!(extract_zip(&archive, &dest, 0).is_err());
    }

    #[test]
    fn refuses_unvetted_download() {
        let error = futures_executor_block();
        assert!(error.contains("not a vetted"));
    }

    #[test]
    fn refuses_wrong_archive_digest() {
        let error = verify_archive_digest("2026.10.19-6bbf2c4", "00".repeat(32).as_str())
            .unwrap_err()
            .to_string();
        assert!(error.contains("did not match the pin"));
    }

    #[test]
    fn accepts_pinned_archive_digest() {
        let os = if cfg!(windows) { "windows" } else { "linux" };
        let arch = crate::cli::paths::host_arch().expect("arch");
        let digest = crate::cli::version::expected_archive_sha256("2026.10.19-6bbf2c4", os, arch)
            .expect("pin");
        verify_archive_digest("2026.10.19-6bbf2c4", digest).unwrap();
    }

    fn futures_executor_block() -> String {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(download_archive("2020.01.01-deadbeef", |_| {}))
            .unwrap_err()
            .to_string()
    }
}
