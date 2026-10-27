use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::cli::paths::home_dir;
use crate::worker::error::WorkerError;

/// Rotating wrapper stdout/stderr file under the worker data dir.
pub const WRAPPER_LOG: &str = "wrapper.log";

/// Isolated `--data-dir` for one named worker. Created on Start.
///
/// Linux: `$XDG_DATA_HOME/worker-dock/workers/<id>/` (default `~/.local/share/worker-dock/workers/<id>/`).
/// Windows: `%LOCALAPPDATA%\StandEasyOps\WorkerDock\workers\<id>\`.
pub fn worker_data_dir(id: &str) -> Result<PathBuf, WorkerError> {
    if Uuid::parse_str(id).is_err() {
        return Err(WorkerError::msg(
            "worker id must be a UUID before it can be used as a data-dir name",
        ));
    }
    Ok(workers_root()?.join(id))
}

pub fn workers_root() -> Result<PathBuf, WorkerError> {
    if cfg!(windows) {
        let local = nonempty_env("LOCALAPPDATA")
            .ok_or_else(|| WorkerError::msg("%LOCALAPPDATA% is not set"))?;
        return Ok(PathBuf::from(local)
            .join("StandEasyOps")
            .join("WorkerDock")
            .join("workers"));
    }
    if let Some(xdg) = nonempty_env("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg).join("worker-dock").join("workers"));
    }
    Ok(home_dir()
        .map_err(|error| WorkerError::msg(error.to_string()))?
        .join(".local")
        .join("share")
        .join("worker-dock")
        .join("workers"))
}

/// Delete contents of the isolated `--data-dir` only (logical worker identity).
/// Refuses a symlink root or a path that is not `workers_root()/<uuid>`.
pub fn wipe_isolated_data_dir(id: &str) -> Result<PathBuf, WorkerError> {
    let dir = worker_data_dir(id)?;
    let root = workers_root()?;
    wipe_isolated_contents(&dir, &root)?;
    crate::privacy::ensure_private_dir(&dir)?;
    Ok(dir)
}

pub(crate) fn wipe_isolated_contents(dir: &Path, root: &Path) -> Result<(), WorkerError> {
    let name = dir
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| WorkerError::msg("refusing a data dir without a basename"))?;
    if Uuid::parse_str(name).is_err() {
        return Err(WorkerError::msg(
            "refusing to wipe a data dir that is not a UUID folder",
        ));
    }
    if dir.parent() != Some(root) {
        return Err(WorkerError::msg(
            "refusing to wipe a data dir outside the worker-dock workers root",
        ));
    }
    if !dir.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() {
        return Err(WorkerError::msg(
            "refusing to wipe a data dir that is a symlink",
        ));
    }
    if !meta.is_dir() {
        return Err(WorkerError::msg("worker data dir is not a directory"));
    }
    for entry in fs::read_dir(dir)? {
        let child = entry?.path();
        let child_meta = fs::symlink_metadata(&child)?;
        if child_meta.file_type().is_symlink() || child_meta.is_file() {
            fs::remove_file(&child)?;
        } else if child_meta.is_dir() {
            fs::remove_dir_all(&child)?;
        }
    }
    Ok(())
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_requires_uuid() {
        assert!(worker_data_dir("../evil").is_err());
        assert!(worker_data_dir("not-a-uuid").is_err());
        let id = "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60";
        let path = worker_data_dir(id).unwrap();
        assert!(path.ends_with(id));
        assert!(
            path.to_string_lossy().contains("worker-dock")
                || path.to_string_lossy().contains("WorkerDock")
        );
    }

    #[test]
    fn wipe_removes_children_and_keeps_the_uuid_dir() {
        let root = tempfile::tempdir().unwrap();
        let id = "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60";
        let dir = root.path().join(id);
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("wrapper.log"), "old identity").unwrap();
        fs::write(dir.join("nested").join("id"), "secret-id").unwrap();
        wipe_isolated_contents(&dir, root.path()).unwrap();
        assert!(dir.is_dir());
        assert!(fs::read_dir(&dir).unwrap().next().is_none());
    }

    #[test]
    fn wipe_refuses_a_path_outside_the_workers_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert!(wipe_isolated_contents(outside.path(), root.path()).is_err());
        assert!(wipe_isolated_contents(&root.path().join("not-a-uuid"), root.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn wipe_refuses_a_symlink_data_dir() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let id = "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60";
        let linked = root.path().join(id);
        std::os::unix::fs::symlink(outside.path(), &linked).unwrap();
        assert!(wipe_isolated_contents(&linked, root.path()).is_err());
        assert!(outside.path().exists());
    }
}
