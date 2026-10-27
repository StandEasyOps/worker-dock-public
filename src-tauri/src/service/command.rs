use std::path::{Path, PathBuf};

use crate::worker::command::personal_start_command;
use crate::worker::config::WorkerRecord;
use crate::worker::error::WorkerError;
use crate::worker::paths::worker_data_dir;
use crate::worker::validate::validate_worker_dirs;

/// Argv for a personal My Machines worker. Never includes `--api-key` or pool flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub agent: PathBuf,
    pub args: Vec<String>,
    pub work_dir: PathBuf,
    pub data_dir: PathBuf,
}

/// Service launch uses the same `startArgv` as `worker_get` / `worker_save`.
/// Worker flags (`--name`, each `--worker-dir`) sit before `start`.
pub fn build_launch_plan(record: &WorkerRecord, agent: &Path) -> Result<LaunchPlan, WorkerError> {
    if uuid::Uuid::parse_str(&record.id).is_err() {
        return Err(WorkerError::msg(
            "Worker id must be a UUID before the supervised job can start.",
        ));
    }
    if record.name.trim().is_empty() {
        return Err(WorkerError::msg(
            "Save a display name before starting the worker.",
        ));
    }
    let dirs = validate_worker_dirs(&record.worker_dirs)?;
    let data_dir = worker_data_dir(&record.id)?;
    let work_dir = PathBuf::from(
        dirs.first()
            .ok_or_else(|| WorkerError::msg("Add at least one worker folder before starting."))?,
    );
    if record.management_port < 1024 {
        return Err(WorkerError::msg(
            "management_port must be 1024 or higher and is bound to 127.0.0.1 only.",
        ));
    }
    let mut launch_record = record.clone();
    launch_record.worker_dirs = dirs;
    let start = personal_start_command(
        &agent.display().to_string(),
        &launch_record,
        &data_dir.display().to_string(),
    )?;

    Ok(LaunchPlan {
        agent: agent.to_path_buf(),
        args: start.start_argv,
        work_dir,
        data_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::config::{SHARE_DESKTOP_VIEW_AND_CONTROL, WorkerRecord};

    fn record() -> WorkerRecord {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        std::mem::forget(dir);
        WorkerRecord::defaults(
            "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60".into(),
            "dev-laptop".into(),
            vec![path],
        )
    }

    #[test]
    fn personal_argv_has_no_pool_or_api_key() {
        let record = record();
        let plan = build_launch_plan(&record, Path::new("/tmp/agent")).unwrap();
        let joined = plan.args.join(" ");
        assert_eq!(plan.args[0], crate::cli::detect::DISABLE_AUTO_UPDATE);
        assert_eq!(plan.args[1], "worker");
        assert_ne!(plan.args.get(2).map(String::as_str), Some("start"));
        assert!(joined.contains("--name dev-laptop"));
        assert!(joined.contains("--idle-release-timeout 0"));
        assert!(joined.contains("--management-addr 127.0.0.1:18741"));
        assert!(joined.ends_with("start --verbose"));
        assert!(!joined.contains("worker start --name"));
        assert!(!joined.contains("--pool"));
        assert!(!joined.contains("--api-key"));
        assert!(!joined.contains("CURSOR_API_KEY"));
        assert!(
            plan.data_dir
                .ends_with("8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60")
        );
    }

    #[test]
    fn two_worker_dirs_appear_before_start() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let a = first
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let b = second
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        std::mem::forget(first);
        std::mem::forget(second);
        let record = WorkerRecord::defaults(
            "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60".into(),
            "my-devbox".into(),
            vec![a.clone(), b.clone()],
        );
        let plan = build_launch_plan(&record, Path::new("/tmp/agent")).unwrap();
        let name_at = plan.args.iter().position(|part| part == "--name").unwrap();
        let start_at = plan.args.iter().position(|part| part == "start").unwrap();
        let first_dir = plan.args.iter().position(|part| part == &a).unwrap();
        let second_dir = plan.args.iter().position(|part| part == &b).unwrap();
        assert!(name_at < start_at);
        assert!(first_dir < start_at);
        assert!(second_dir < start_at);
        assert_eq!(plan.args[first_dir - 1], "--worker-dir");
        assert_eq!(plan.args[second_dir - 1], "--worker-dir");
    }

    #[test]
    fn linux_optional_flags_only_when_set() {
        let mut record = record();
        record.computer_use = true;
        record.display = ":0".into();
        record.connect_workspace = true;
        record.identity_socket = true;
        record.share_desktop = true;
        record.share_desktop_mode = SHARE_DESKTOP_VIEW_AND_CONTROL.into();
        let plan = build_launch_plan(&record, Path::new("/tmp/agent")).unwrap();
        let joined = plan.args.join(" ");
        let start_at = plan.args.iter().position(|part| part == "start").unwrap();
        let computer = plan
            .args
            .iter()
            .position(|part| part == "--computer-use")
            .unwrap();
        assert!(computer < start_at);
        assert!(joined.contains("--computer-use"));
        assert!(joined.contains("--display :0"));
        if cfg!(windows) {
            assert!(!joined.contains("--connect-workspace"));
        } else {
            assert!(joined.contains("--connect-workspace"));
        }
        assert!(joined.contains("--identity-socket"));
        assert!(joined.contains("--share-desktop view_and_control"));
    }
}
