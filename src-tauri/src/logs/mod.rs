//! Live tail of wrapper.log and, when present, the CLI session latest.log.
//!
//! Paths are computed, never taken from the UI. Do not dump the whole file.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::worker::config::load_records;
use crate::worker::error::WorkerError;
use crate::worker::paths::{WRAPPER_LOG, worker_data_dir};

pub(crate) mod claims;

pub const TAIL_BYTES: u64 = 64 * 1024;
pub const TAIL_LINES: usize = 1000;
const SESSION_LOG_NAME: &str = "latest.log";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsSnapshot {
    pub folder: String,
    pub sources: Vec<LogSourceView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSourceView {
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub text: String,
    pub truncated: bool,
}

#[tauri::command]
pub fn logs_read() -> Result<LogsSnapshot, WorkerError> {
    let record = primary_record()?;
    let folder = worker_data_dir(&record.id)?;
    let wrapper_path = join_allowed(&folder, WRAPPER_LOG)?;
    let mut sources = vec![source_from_path("wrapper", &wrapper_path)];

    if let Some(session) = session_latest_log() {
        sources.push(source_from_path("session", &session));
    }
    if let Some(startup) = windows_startup_log() {
        sources.push(source_from_path("startup", &startup));
    }

    Ok(LogsSnapshot {
        folder: folder.display().to_string(),
        sources,
    })
}

#[tauri::command]
pub async fn logs_open_folder() -> Result<String, WorkerError> {
    let record = primary_record()?;
    let folder = worker_data_dir(&record.id)?;
    if !folder.is_dir() {
        return Err(WorkerError::msg(
            "Start the worker once so the data directory exists.",
        ));
    }
    let path = folder.display().to_string();
    tokio::task::spawn_blocking(move || open::that_detached(&path))
        .await
        .map_err(|error| WorkerError::msg(format!("open log folder failed: {error}")))?
        .map_err(|error| WorkerError::msg(format!("open log folder failed: {error}")))?;
    Ok(folder.display().to_string())
}

pub fn tail_existing(path: &Path) -> Result<String, WorkerError> {
    let (text, _) = tail_file(path)?;
    Ok(text)
}

fn source_from_path(name: &str, path: &Path) -> LogSourceView {
    match tail_file(path) {
        Ok((text, truncated)) => LogSourceView {
            name: name.to_string(),
            path: path.display().to_string(),
            exists: true,
            text: crate::redact::redact(&text),
            truncated,
        },
        Err(_) => LogSourceView {
            name: name.to_string(),
            path: path.display().to_string(),
            exists: false,
            text: String::new(),
            truncated: false,
        },
    }
}

fn tail_file(path: &Path) -> Result<(String, bool), WorkerError> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(WorkerError::msg("log path is not a file"));
    }
    let mut file = File::open(path)?;
    let mut byte_truncated = false;
    if meta.len() > TAIL_BYTES {
        file.seek(SeekFrom::End(-(TAIL_BYTES as i64)))?;
        byte_truncated = true;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if byte_truncated {
        if let Some(idx) = bytes.iter().position(|byte| *byte == b'\n') {
            bytes = bytes[idx + 1..].to_vec();
        }
    }
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let lines: Vec<&str> = raw.lines().collect();
    let line_truncated = lines.len() > TAIL_LINES;
    let text = if line_truncated {
        lines[lines.len() - TAIL_LINES..].join("\n")
    } else {
        raw
    };
    Ok((text, byte_truncated || line_truncated))
}

fn join_allowed(root: &Path, name: &str) -> Result<PathBuf, WorkerError> {
    if name != WRAPPER_LOG && name != SESSION_LOG_NAME {
        return Err(WorkerError::msg("refusing an unknown log file name"));
    }
    if name.contains(['/', '\\']) || name.contains("..") {
        return Err(WorkerError::msg("refusing a log path with separators"));
    }
    let path = root.join(name);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(WorkerError::msg(
            "refusing a log path that leaves the data dir",
        ));
    }
    if path.file_name().and_then(|value| value.to_str()) != Some(name) {
        return Err(WorkerError::msg(
            "refusing a log path that does not stay a basename",
        ));
    }
    Ok(path)
}

pub fn session_log_root(uid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/cursor-agent-logs-{uid}"))
}

pub fn resolve_under(path: &Path, root: &Path) -> Result<PathBuf, WorkerError> {
    let canon = path
        .canonicalize()
        .map_err(|error| WorkerError::msg(format!("log path: {error}")))?;
    let root = root
        .canonicalize()
        .map_err(|error| WorkerError::msg(format!("log root: {error}")))?;
    if !canon.starts_with(&root) {
        return Err(WorkerError::msg(
            "refusing a session log that leaves the expected directory",
        ));
    }
    Ok(canon)
}

fn session_latest_log() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let uid = linux_uid()?;
        let root = session_log_root(uid);
        let latest = root.join(SESSION_LOG_NAME);
        if !latest.exists() {
            return Some(latest);
        }
        match resolve_under(&latest, &root) {
            Ok(path) => Some(path),
            Err(_) => None,
        }
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn windows_startup_log() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let local = std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|value| !value.trim().is_empty())?;
    Some(
        PathBuf::from(local)
            .join("cursor-agent")
            .join("worker-startup.log"),
    )
}

#[cfg(unix)]
fn linux_uid() -> Option<u32> {
    let text = fs::read_to_string("/proc/self/status").ok()?;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("Uid:") else {
            continue;
        };
        return rest.split_whitespace().next()?.parse().ok();
    }
    None
}

fn primary_record() -> Result<crate::worker::config::WorkerRecord, WorkerError> {
    let record = load_records()?
        .into_iter()
        .next()
        .ok_or_else(|| WorkerError::msg("Save the personal worker first."))?;
    if uuid::Uuid::parse_str(&record.id).is_err() {
        return Err(WorkerError::msg(
            "Worker id must be a UUID before logs can be opened.",
        ));
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn join_allowed_rejects_traversal() {
        let root = PathBuf::from("/tmp/worker-dock-logs-test");
        assert!(join_allowed(&root, WRAPPER_LOG).is_ok());
        assert!(join_allowed(&root, "../wrapper.log").is_err());
        assert!(join_allowed(&root, "latest.log").is_ok());
        assert!(join_allowed(&root, "other.log").is_err());
        assert!(join_allowed(&root, "wrapper.log/../secret").is_err());
    }

    #[test]
    fn session_root_uses_uid_prefix_only() {
        let root = session_log_root(1000);
        assert_eq!(root, PathBuf::from("/tmp/cursor-agent-logs-1000"));
        assert!(
            root.to_string_lossy()
                .starts_with("/tmp/cursor-agent-logs-")
        );
        assert!(!root.to_string_lossy().contains(".."));
    }

    #[test]
    fn ui_log_text_redacts_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrapper.log");
        fs::write(
            &path,
            "Authorization: Bearer eyJabc.def.ghi\nCURSOR_API_KEY=abcd-known-secret\nready\n",
        )
        .unwrap();
        let source = source_from_path("wrapper", &path);
        assert!(source.exists);
        assert!(source.text.contains("ready"));
        assert!(!source.text.contains("eyJabc.def.ghi"));
        assert!(!source.text.contains("abcd-known-secret"));
    }

    #[test]
    fn tail_keeps_the_end_of_a_large_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrapper.log");
        let mut file = File::create(&path).unwrap();
        let line = "x".repeat(80);
        for index in 0..2000 {
            writeln!(file, "{index}:{line}").unwrap();
        }
        file.flush().unwrap();
        let (text, truncated) = tail_file(&path).unwrap();
        assert!(truncated);
        assert!(text.contains("1999:"));
        assert!(!text.lines().any(|line| line.starts_with("0:")));
        assert!(text.lines().count() <= TAIL_LINES);
        assert!(text.len() as u64 <= TAIL_BYTES);
    }

    #[test]
    fn resolve_under_rejects_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let inside = root.path().join("latest.log");
        fs::write(&inside, "ok").unwrap();
        let escaped = outside.path().join("secret.log");
        fs::write(&escaped, "secret").unwrap();
        assert_eq!(
            resolve_under(&inside, root.path()).unwrap(),
            inside.canonicalize().unwrap()
        );
        assert!(resolve_under(&escaped, root.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_under_rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("secret.log");
        fs::write(&target, "secret").unwrap();
        let link = root.path().join("latest.log");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(resolve_under(&link, root.path()).is_err());
    }
}
