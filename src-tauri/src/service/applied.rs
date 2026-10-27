use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::worker::config::WorkerRecord;
use crate::worker::error::WorkerError;
use crate::worker::paths::worker_data_dir;
use crate::worker::validate::validate_worker_dirs;

const APPLIED_FILE: &str = "applied-launch.json";

/// Name and `--worker-dir` values last exec'd by the supervised job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedLaunch {
    pub name: String,
    pub worker_dirs: Vec<String>,
}

pub fn applied_path(id: &str) -> Result<PathBuf, WorkerError> {
    Ok(worker_data_dir(id)?.join(APPLIED_FILE))
}

pub fn write_from_args(id: &str, args: &[String]) -> Result<(), WorkerError> {
    let (name, worker_dirs) = name_and_dirs_from_argv(args);
    if name.is_empty() || worker_dirs.is_empty() {
        return Err(WorkerError::msg(
            "startArgv must include --name and at least one --worker-dir before it can be recorded",
        ));
    }
    let dir = worker_data_dir(id)?;
    crate::privacy::ensure_private_dir(&dir)?;
    let payload = AppliedLaunch { name, worker_dirs };
    let text = serde_json::to_string_pretty(&payload)
        .map_err(|error| WorkerError::msg(format!("could not write applied launch: {error}")))?;
    crate::privacy::write_private_file(&applied_path(id)?, text)?;
    Ok(())
}

pub fn read(id: &str) -> Option<AppliedLaunch> {
    let path = applied_path(id).ok()?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn name_and_dirs_from_argv(args: &[String]) -> (String, Vec<String>) {
    let mut name = String::new();
    let mut worker_dirs = Vec::new();
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "start" => break,
            "--name" if idx + 1 < args.len() => {
                name = args[idx + 1].clone();
                idx += 2;
            }
            "--worker-dir" if idx + 1 < args.len() => {
                worker_dirs.push(args[idx + 1].clone());
                idx += 2;
            }
            _ => idx += 1,
        }
    }
    (name, worker_dirs)
}

pub fn restart_required(record: &WorkerRecord, state: &str, applied: Option<&AppliedLaunch>) -> bool {
    if !matches!(state, "running" | "starting") {
        return false;
    }
    let Some(applied) = applied else {
        return true;
    };
    if applied.name != record.name {
        return true;
    }
    let current =
        validate_worker_dirs(&record.worker_dirs).unwrap_or_else(|_| record.worker_dirs.clone());
    applied.worker_dirs != current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_name_and_repeated_dirs_before_start() {
        let args = vec![
            "--disable-auto-update".into(),
            "worker".into(),
            "--name".into(),
            "dev-laptop".into(),
            "--worker-dir".into(),
            "/home/dev/repos/app".into(),
            "--worker-dir".into(),
            "/home/dev/repos/infra".into(),
            "--data-dir".into(),
            "/tmp/data".into(),
            "start".into(),
            "--verbose".into(),
        ];
        let (name, dirs) = name_and_dirs_from_argv(&args);
        assert_eq!(name, "dev-laptop");
        assert_eq!(dirs, vec!["/home/dev/repos/app", "/home/dev/repos/infra"]);
    }

    #[test]
    fn restart_required_when_running_name_or_dirs_change() {
        let record = WorkerRecord::defaults(
            "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60".into(),
            "new-name".into(),
            vec!["/tmp/a".into()],
        );
        let applied = AppliedLaunch {
            name: "old-name".into(),
            worker_dirs: vec!["/tmp/a".into()],
        };
        assert!(restart_required(&record, "running", Some(&applied)));
        assert!(restart_required(&record, "starting", None));
        assert!(!restart_required(&record, "stopped", Some(&applied)));
        let matching = AppliedLaunch {
            name: "new-name".into(),
            worker_dirs: vec!["/tmp/a".into()],
        };
        assert!(!restart_required(&record, "running", Some(&matching)));
    }
}
