//! User-only permissions for config, data dirs, and logs.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

/// Create `path` (and parents we create under it) and restrict the leaf to 0700
/// / the current Windows user. Does not chmod `$HOME` or `~/.config`.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    restrict_path(path)
}

/// Write `contents` and set the file to 0600 / the current Windows user.
pub fn write_private_file(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    write_private_bytes(path, contents.as_ref())?;
    restrict_path(path)
}

/// Append-only log file created as 0600 / current Windows user.
pub fn open_private_append(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            ensure_private_dir(parent)?;
        }
    }
    let file = private_append_options().open(path)?;
    restrict_path(path)?;
    Ok(file)
}

/// Tighten an existing tree left behind by older builds (0775/0664 leftovers).
/// Directories become 0700 and files 0600. Symlinks are skipped. Missing path
/// is a no-op so startup can call this before the first save.
pub fn tighten_tree(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    restrict_path(path)?;
    if !meta.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let child = entry?.path();
        tighten_tree(&child)?;
    }
    Ok(())
}

/// Restrict an existing path. Best-effort on Windows when `icacls` is missing
/// (user-profile ACLs already default to the owner).
pub fn restrict_path(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        restrict_windows(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

fn write_private_bytes(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut file = private_write_options().open(path)?;
    file.write_all(contents)?;
    file.flush()?;
    Ok(())
}

fn private_write_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn private_append_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

#[cfg(windows)]
fn restrict_windows(path: &Path) -> io::Result<()> {
    let account = current_windows_account()?;
    let grant = if path.is_dir() {
        format!("{account}:(OI)(CI)F")
    } else {
        format!("{account}:(F)")
    };
    let mut command = std::process::Command::new("icacls");
    command
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(&grant);
    crate::process::hide_std_window(&mut command);
    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(io::Error::other(if stderr.is_empty() {
            "icacls failed to restrict the path to the current user".to_string()
        } else {
            stderr
        }))
    }
}

#[cfg(windows)]
fn current_windows_account() -> io::Result<String> {
    let user = std::env::var("USERNAME").map_err(|_| {
        io::Error::new(io::ErrorKind::NotFound, "%USERNAME% is not set")
    })?;
    if !is_safe_account_part(&user) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing an unsafe %USERNAME% when setting ACLs",
        ));
    }
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    if !domain.is_empty() {
        if !is_safe_account_part(&domain) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing an unsafe %USERDOMAIN% when setting ACLs",
            ));
        }
        return Ok(format!("{domain}\\{user}"));
    }
    Ok(user)
}

pub fn is_safe_account_part(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn account_parts_reject_metacharacters() {
        assert!(is_safe_account_part("alice"));
        assert!(is_safe_account_part("ALICE-PC"));
        assert!(!is_safe_account_part(""));
        assert!(!is_safe_account_part("alice:admin"));
        assert!(!is_safe_account_part("alice&(whoami)"));
    }

    #[cfg(unix)]
    #[test]
    fn private_dir_and_file_are_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let folder = dir.path().join("data");
        ensure_private_dir(&folder).unwrap();
        assert_eq!(fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o700);
        let file = folder.join("config.toml");
        write_private_file(&file, "schema_version = 1\n").unwrap();
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read_to_string(&file).unwrap(), "schema_version = 1\n");
        let log = folder.join("wrapper.log");
        let handle = open_private_append(&log).unwrap();
        drop(handle);
        assert_eq!(fs::metadata(&log).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn tighten_tree_fixes_legacy_modes() {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let dir = tempdir().unwrap();
        let root = dir.path().join("worker-dock");
        let workers = root.join("workers").join("8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60");
        fs::create_dir_all(&workers).unwrap();
        let config = root.join("config.toml");
        OpenOptions::new()
            .create(true)
            .write(true)
            .mode(0o664)
            .open(&config)
            .unwrap();
        let log = workers.join("wrapper.log");
        OpenOptions::new()
            .create(true)
            .write(true)
            .mode(0o664)
            .open(&log)
            .unwrap();
        for path in [&root, workers.parent().unwrap(), &workers] {
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o775);
            fs::set_permissions(path, permissions).unwrap();
        }
        tighten_tree(&root).unwrap();
        assert_eq!(fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(workers.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(fs::metadata(&workers).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&config).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&log).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
