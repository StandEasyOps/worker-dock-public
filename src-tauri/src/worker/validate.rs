use std::fs;
use std::path::{Path, PathBuf};

use crate::worker::error::WorkerError;

pub const MAX_WORKER_DIRS: usize = 20;
pub const MAX_NAME_LEN: usize = 64;

pub fn validate_name(name: &str) -> Result<String, WorkerError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(WorkerError::msg("Enter a display name for this personal worker."));
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(WorkerError::msg(format!(
            "The worker name must be {MAX_NAME_LEN} characters or fewer."
        )));
    }
    if name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | '\0')) {
        return Err(WorkerError::msg(
            "The worker name cannot contain slashes or control characters.",
        ));
    }
    Ok(name.to_string())
}

/// `--display` is a single argv token. Reject shell metacharacters and junk
/// so a hand-edited config cannot smuggle extra flags.
pub fn validate_display(display: &str) -> Result<String, WorkerError> {
    let display = display.trim();
    if display.is_empty() {
        return Ok(String::new());
    }
    if display.chars().count() > 64 {
        return Err(WorkerError::msg(
            "The display name must be 64 characters or fewer.",
        ));
    }
    if display.starts_with('-') {
        return Err(WorkerError::msg(
            "The display name cannot start with '-' (it would look like a flag).",
        ));
    }
    if display.chars().any(|c| {
        c.is_control()
            || c.is_whitespace()
            || matches!(c, ';' | '|' | '&' | '$' | '`' | '\'' | '"' | '\\' | '/' | '\0')
    }) {
        return Err(WorkerError::msg(
            "The display name can only be a screen identifier such as :0.",
        ));
    }
    Ok(display.to_string())
}

pub fn is_safe_session_user(user: &str) -> bool {
    crate::privacy::is_safe_account_part(user)
}

pub fn suggested_name() -> String {
    hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .map(|raw| sanitize_suggested_name(&raw))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "dev-machine".to_string())
}

pub fn sanitize_suggested_name(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
            out.push(c);
        } else if matches!(c, ' ' | '/' | '\\') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(MAX_NAME_LEN).collect()
}

/// Require existing absolute directories, max 20. Canonicalize so later argv
/// construction is not a relative path. First entry is assignment identity.
pub fn validate_worker_dirs(dirs: &[String]) -> Result<Vec<String>, WorkerError> {
    if dirs.is_empty() {
        return Err(WorkerError::msg(
            "Add at least one existing folder. The first folder is the assignment identity.",
        ));
    }
    if dirs.len() > MAX_WORKER_DIRS {
        return Err(WorkerError::msg(format!(
            "A personal worker can expose at most {MAX_WORKER_DIRS} folders."
        )));
    }
    let mut out = Vec::with_capacity(dirs.len());
    for raw in dirs {
        let path = validate_one_dir(raw)?;
        let display = path.to_string_lossy().to_string();
        if out.iter().any(|existing: &String| existing == &display) {
            return Err(WorkerError::msg(format!(
                "Folder listed more than once: {display}"
            )));
        }
        out.push(display);
    }
    Ok(out)
}

pub fn dir_status(path: &str) -> (bool, bool) {
    match fs::metadata(path) {
        Ok(meta) => (true, meta.is_dir()),
        Err(_) => (false, false),
    }
}

fn validate_one_dir(raw: &str) -> Result<PathBuf, WorkerError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(WorkerError::msg("A worker folder path is empty."));
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(WorkerError::msg(format!(
            "Worker folders must be absolute paths: {trimmed}"
        )));
    }
    let meta = fs::metadata(path).map_err(|_| {
        WorkerError::msg(format!("Worker folder does not exist: {trimmed}"))
    })?;
    if !meta.is_dir() {
        return Err(WorkerError::msg(format!(
            "Worker folder is not a directory: {trimmed}"
        )));
    }
    path.canonicalize().map_err(|error| {
        WorkerError::msg(format!("Could not resolve worker folder {trimmed}: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn name_rules() {
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name("dev/laptop").is_err());
        assert!(validate_name("dev\\laptop").is_err());
        assert_eq!(validate_name("  dev laptop  ").unwrap(), "dev laptop");
        assert!(validate_name(&"n".repeat(MAX_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn display_rules() {
        assert_eq!(validate_display("  :0  ").unwrap(), ":0");
        assert_eq!(validate_display("").unwrap(), "");
        assert!(validate_display("--display").is_err());
        assert!(validate_display(":0;id").is_err());
        assert!(validate_display("wayland-0 extra").is_err());
    }

    #[test]
    fn sanitizes_hostname() {
        assert_eq!(sanitize_suggested_name("my laptop"), "my-laptop");
        assert_eq!(sanitize_suggested_name("dev.local"), "dev.local");
        assert_eq!(sanitize_suggested_name("///"), "");
    }

    #[test]
    fn dirs_must_be_existing_absolute() {
        let dir = tempdir().unwrap();
        let abs = dir.path().to_string_lossy().to_string();
        assert!(validate_worker_dirs(&[]).is_err());
        assert!(validate_worker_dirs(&["relative/path".into()]).is_err());
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(validate_worker_dirs(&[file.to_string_lossy().to_string()]).is_err());
        let missing = dir.path().join("nope");
        assert!(validate_worker_dirs(&[missing.to_string_lossy().to_string()]).is_err());
        let ok = validate_worker_dirs(&[abs.clone()]).unwrap();
        assert_eq!(ok.len(), 1);
        assert!(validate_worker_dirs(&[abs.clone(), abs]).is_err());
    }

    #[test]
    fn rejects_more_than_twenty_dirs() {
        let dirs = (0..21).map(|i| format!("/tmp/w{i}")).collect::<Vec<_>>();
        assert!(validate_worker_dirs(&dirs).is_err());
    }
}
