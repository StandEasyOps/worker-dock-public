use std::path::{Path, PathBuf};

use crate::cli::error::CliError;
use crate::cli::version::PINNED_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    Linux,
    Windows,
}

pub fn host_os() -> HostOs {
    if cfg!(windows) {
        HostOs::Windows
    } else {
        HostOs::Linux
    }
}

pub fn host_arch() -> Result<&'static str, CliError> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("arm64"),
        other => Err(CliError::msg(format!(
            "unsupported architecture {other}; worker-dock targets x64 and arm64"
        ))),
    }
}

pub fn home_dir() -> Result<PathBuf, CliError> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| CliError::msg("home directory is not set"))
}

pub fn config_dir() -> Result<PathBuf, CliError> {
    if let Some(xdg) = nonempty_env("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("worker-dock"));
    }
    if cfg!(windows) {
        let appdata = nonempty_env("APPDATA")
            .ok_or_else(|| CliError::msg("%APPDATA% is not set"))?;
        return Ok(PathBuf::from(appdata)
            .join("StandEasyOps")
            .join("WorkerDock"));
    }
    Ok(home_dir()?.join(".config").join("worker-dock"))
}

pub fn config_path() -> Result<PathBuf, CliError> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn cache_dir() -> Result<PathBuf, CliError> {
    if let Some(xdg) = nonempty_env("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(xdg).join("worker-dock").join("cli"));
    }
    if cfg!(windows) {
        let local = nonempty_env("LOCALAPPDATA")
            .ok_or_else(|| CliError::msg("%LOCALAPPDATA% is not set"))?;
        return Ok(PathBuf::from(local)
            .join("StandEasyOps")
            .join("WorkerDock")
            .join("cache")
            .join("cli"));
    }
    Ok(home_dir()?.join(".cache").join("worker-dock").join("cli"))
}

pub fn official_versions_dir() -> Result<PathBuf, CliError> {
    if cfg!(windows) {
        let local = nonempty_env("LOCALAPPDATA")
            .ok_or_else(|| CliError::msg("%LOCALAPPDATA% is not set"))?;
        return Ok(PathBuf::from(local).join("cursor-agent").join("versions"));
    }
    Ok(home_dir()?
        .join(".local")
        .join("share")
        .join("cursor-agent")
        .join("versions"))
}

pub fn official_version_dir(version: &str) -> Result<PathBuf, CliError> {
    Ok(official_versions_dir()?.join(version))
}

pub fn linux_shim_dir() -> Result<PathBuf, CliError> {
    Ok(home_dir()?.join(".local").join("bin"))
}

pub fn windows_shim_dir() -> Result<PathBuf, CliError> {
    let local = nonempty_env("LOCALAPPDATA")
        .ok_or_else(|| CliError::msg("%LOCALAPPDATA% is not set"))?;
    Ok(PathBuf::from(local).join("cursor-agent"))
}

/// Official download used by https://cursor.com/install (Linux) and ?win32=true (Windows).
pub fn official_archive_url(version: &str, os: HostOs, arch: &str) -> Result<url::Url, CliError> {
    if !crate::cli::version::is_cli_version(version) {
        return Err(CliError::msg("refusing to build a download URL for an invalid version"));
    }
    if !matches!(arch, "x64" | "arm64") {
        return Err(CliError::msg(format!("unsupported archive architecture {arch}")));
    }
    let (platform, filename) = match os {
        HostOs::Linux => ("linux", "agent-cli-package.tar.gz"),
        HostOs::Windows => ("windows", "agent-cli-package.zip"),
    };
    let raw = format!("https://downloads.cursor.com/lab/{version}/{platform}/{arch}/{filename}");
    let parsed = url::Url::parse(&raw)?;
    assert_https_host(&parsed, "downloads.cursor.com")?;
    Ok(parsed)
}

pub fn official_install_script_url(os: HostOs) -> Result<url::Url, CliError> {
    let raw = match os {
        HostOs::Linux => "https://cursor.com/install",
        HostOs::Windows => "https://cursor.com/install?win32=true",
    };
    let parsed = url::Url::parse(raw)?;
    assert_https_host(&parsed, "cursor.com")?;
    Ok(parsed)
}

pub fn assert_https_host(url: &url::Url, host: &str) -> Result<(), CliError> {
    if url.scheme() != "https" {
        return Err(CliError::msg(format!("refusing non-HTTPS URL: {url}")));
    }
    if url.host_str() != Some(host) {
        return Err(CliError::msg(format!(
            "refusing unexpected host {}; expected {host}",
            url.host_str().unwrap_or("none")
        )));
    }
    Ok(())
}

pub fn binary_name_in_version_dir(os: HostOs) -> &'static str {
    match os {
        HostOs::Linux => "cursor-agent",
        HostOs::Windows => "cursor-agent.exe",
    }
}

pub fn look_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    for dir in &dirs {
        let candidate = dir.join(name);
        if is_usable_file(&candidate) {
            return Some(crate::process::prefer_hidden_agent(&candidate));
        }
        if cfg!(windows) {
            let exe = dir.join(format!("{name}.exe"));
            if is_usable_file(&exe) {
                return Some(exe);
            }
        }
    }
    // Windows: only fall back to a console-launching .cmd when no .exe exists
    // anywhere on PATH. prefer_hidden_agent still remaps to a sibling .exe.
    if cfg!(windows) {
        for dir in &dirs {
            let cmd = dir.join(format!("{name}.cmd"));
            if is_usable_file(&cmd) {
                return Some(crate::process::prefer_hidden_agent(&cmd));
            }
        }
    }
    None
}

pub fn is_usable_file(path: &Path) -> bool {
    path.is_file()
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn default_search_paths() -> Result<Vec<PathBuf>, CliError> {
    let mut paths = Vec::new();
    let os = host_os();
    let pinned_dir = official_version_dir(PINNED_VERSION)?;
    paths.push(pinned_dir.join(binary_name_in_version_dir(os)));
    match os {
        HostOs::Linux => {
            paths.push(linux_shim_dir()?.join("agent"));
        }
        HostOs::Windows => {
            let shims = windows_shim_dir()?;
            paths.push(shims.join("agent.exe"));
            paths.push(shims.join("cursor-agent.exe"));
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_archive_url_is_official() {
        let url = official_archive_url("2026.10.19-6bbf2c4", HostOs::Linux, "x64").unwrap();
        assert_eq!(
            url.as_str(),
            "https://downloads.cursor.com/lab/2026.10.19-6bbf2c4/linux/x64/agent-cli-package.tar.gz"
        );
    }

    #[test]
    fn windows_archive_url_is_official() {
        let url = official_archive_url("2026.10.19-6bbf2c4", HostOs::Windows, "x64").unwrap();
        assert_eq!(
            url.as_str(),
            "https://downloads.cursor.com/lab/2026.10.19-6bbf2c4/windows/x64/agent-cli-package.zip"
        );
    }

    #[test]
    fn rejects_path_version() {
        assert!(official_archive_url("../evil", HostOs::Linux, "x64").is_err());
        assert!(official_archive_url("2026.10.19-6bbf2c4", HostOs::Linux, "ppc").is_err());
    }

    #[test]
    fn windows_search_lists_exe_before_cmd() {
        let shims = PathBuf::from(r"C:\Users\dev\AppData\Local\cursor-agent");
        // default_search_paths on this host is OS-specific; the Windows list
        // must never include agent.cmd (that is what opened visible consoles).
        if cfg!(windows) {
            let paths = default_search_paths().unwrap();
            assert!(
                paths.iter().all(|path| {
                    path.extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                }),
                "Windows default search must be .exe only, got {paths:?}"
            );
        }
        let _ = shims;
    }
}
