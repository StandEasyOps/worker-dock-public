//! One named personal worker: config, per-user start/stop, and status chips.
//!
//! Multiple workers, Team Pools, and diagnostics are out of scope.

pub(crate) mod command;
pub(crate) mod config;
pub(crate) mod error;
pub(crate) mod git;
pub(crate) mod paths;
pub(crate) mod validate;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::cli::config::{configured_binary, load as load_cli_config};
use crate::cli::paths::config_path;
use crate::logs::claims::{self, RecentAgent};
use crate::service::{self, DirectWorker, RuntimeView, Watchdog};
use command::{StartCommand, personal_start_command};
use config::{
    WorkerRecord, default_suggested_name, describe_dir, load_records, save_primary_worker,
};
use error::WorkerError;
use git::describe_origin;
use paths::worker_data_dir;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSnapshot {
    pub configured: bool,
    pub extra_worker_count: usize,
    pub suggested_name: String,
    pub config_path: String,
    pub worker: Option<WorkerView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerView {
    pub id: String,
    pub name: String,
    pub worker_dirs: Vec<WorkerDirView>,
    pub management_port: u16,
    pub idle_release_timeout: u32,
    pub data_dir: String,
    pub agent_path: String,
    pub start_argv: Vec<String>,
    pub command_line: String,
    pub runtime: RuntimeView,
    pub recent_agents: Vec<RecentAgent>,
    pub current_agent_bc_id: Option<String>,
    pub can_reset_identity: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerDirView {
    pub path: String,
    pub exists: bool,
    pub is_directory: bool,
    pub git_remote: Option<String>,
    pub has_remote: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSaveRequest {
    pub name: String,
    pub worker_dirs: Vec<String>,
}

#[tauri::command]
pub async fn worker_get(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_save(
    request: WorkerSaveRequest,
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    save_primary_worker(&request.name, &request.worker_dirs)?;
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_pick_dir() -> Result<Option<String>, WorkerError> {
    tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Add a worker folder")
            .pick_folder()
            .map(|path| path.to_string_lossy().to_string())
    })
    .await
    .map_err(|error| WorkerError::msg(format!("folder dialog failed: {error}")))
}

#[tauri::command]
pub async fn worker_preview_command(
    request: WorkerSaveRequest,
) -> Result<StartCommand, WorkerError> {
    let records = load_records()?;
    let mut record = records
        .first()
        .cloned()
        .unwrap_or_else(|| WorkerRecord::defaults(String::new(), String::new(), Vec::new()));
    record.name = request.name.trim().to_string();
    record.worker_dirs = request
        .worker_dirs
        .into_iter()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .collect();
    let data_dir = if uuid::Uuid::parse_str(&record.id).is_ok() {
        worker_data_dir(&record.id)?.display().to_string()
    } else {
        String::new()
    };
    personal_start_command(&resolve_agent_path(), &record, &data_dir)
}

#[tauri::command]
pub fn worker_open_config_folder() -> Result<String, WorkerError> {
    let path = config_path().map_err(|error| WorkerError::msg(error.to_string()))?;
    let folder = path.parent().unwrap_or(&path);
    open::that(folder)?;
    Ok(folder.display().to_string())
}

async fn snapshot(
    direct: &DirectWorker,
    watchdog: &Watchdog,
) -> Result<WorkerSnapshot, WorkerError> {
    let records = load_records()?;
    let extra_worker_count = records.len().saturating_sub(1);
    let config_file = config_path()
        .map(|path| path.display().to_string())
        .map_err(|error| WorkerError::msg(error.to_string()))?;
    let worker = match records.first() {
        Some(record)
            if !record.id.is_empty()
                || !record.name.is_empty()
                || !record.worker_dirs.is_empty() =>
        {
            Some(to_view(record, direct, watchdog).await?)
        }
        _ => None,
    };
    Ok(WorkerSnapshot {
        configured: worker.is_some(),
        extra_worker_count,
        suggested_name: default_suggested_name(),
        config_path: config_file,
        worker,
    })
}

async fn to_view(
    record: &WorkerRecord,
    direct: &DirectWorker,
    watchdog: &Watchdog,
) -> Result<WorkerView, WorkerError> {
    let can_reset_identity = uuid::Uuid::parse_str(&record.id).is_ok();
    let data_dir = if can_reset_identity {
        worker_data_dir(&record.id)?.display().to_string()
    } else {
        String::new()
    };
    let start = personal_start_command(&resolve_agent_path(), record, &data_dir).ok();
    let mut runtime = service::runtime_for(record, direct);
    if can_reset_identity {
        service::status::enrich(&mut runtime, record.management_port, &record.id).await;
    }
    watchdog.apply_to(&mut runtime);
    let recent = claims::recent_from_data_dir(&data_dir);
    let current_agent_bc_id = if worker_is_busy(&runtime, !recent.is_empty()) {
        recent.first().map(|agent| agent.bc_id.clone())
    } else {
        None
    };
    Ok(WorkerView {
        id: record.id.clone(),
        name: record.name.clone(),
        worker_dirs: record
            .worker_dirs
            .iter()
            .map(|path| {
                let (exists, is_directory) = describe_dir(path);
                let remote = if exists && is_directory {
                    describe_origin(path)
                } else {
                    crate::worker::git::GitRemoteInfo {
                        url: None,
                        has_remote: false,
                    }
                };
                WorkerDirView {
                    path: path.clone(),
                    exists,
                    is_directory,
                    git_remote: remote.url,
                    has_remote: remote.has_remote,
                }
            })
            .collect(),
        management_port: record.management_port,
        idle_release_timeout: record.idle_release_timeout,
        data_dir,
        agent_path: start
            .as_ref()
            .map(|command| command.agent_path.clone())
            .unwrap_or_else(resolve_agent_path),
        start_argv: start
            .as_ref()
            .map(|command| command.start_argv.clone())
            .unwrap_or_default(),
        command_line: start
            .map(|command| command.command_line)
            .unwrap_or_default(),
        runtime,
        recent_agents: recent,
        current_agent_bc_id,
        can_reset_identity,
    })
}

fn worker_is_busy(runtime: &RuntimeView, has_claim: bool) -> bool {
    if runtime.session_active == Some(true) {
        return true;
    }
    if runtime.session_active == Some(false) {
        return false;
    }
    if !has_claim {
        return false;
    }
    matches!(runtime.run_mode.as_str(), "service" | "direct")
        || matches!(runtime.headline.as_str(), "ready" | "starting" | "stopping")
}

fn resolve_agent_path() -> String {
    load_cli_config()
        .ok()
        .and_then(|config| configured_binary(&config).map(|path| path.display().to_string()))
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| "agent".to_string())
}

#[tauri::command]
pub async fn worker_install(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    let handle = (*direct).clone();
    tokio::task::spawn_blocking(move || service::install_primary(&handle))
        .await
        .map_err(|error| WorkerError::msg(format!("install task failed: {error}")))??;
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_uninstall(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    watchdog.clear();
    tokio::task::spawn_blocking(service::uninstall_primary)
        .await
        .map_err(|error| WorkerError::msg(format!("uninstall task failed: {error}")))??;
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_start(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    require_ready().await?;
    let handle = (*direct).clone();
    tokio::task::spawn_blocking(move || service::start_primary(&handle))
        .await
        .map_err(|error| WorkerError::msg(format!("start task failed: {error}")))??;
    watchdog.intend_service();
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_stop(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    watchdog.clear();
    let handle = (*direct).clone();
    tokio::task::spawn_blocking(move || service::stop_primary(&handle))
        .await
        .map_err(|error| WorkerError::msg(format!("stop task failed: {error}")))??;
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_restart(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    require_ready().await?;
    let handle = (*direct).clone();
    tokio::task::spawn_blocking(move || service::restart_primary(&handle))
        .await
        .map_err(|error| WorkerError::msg(format!("restart task failed: {error}")))??;
    watchdog.intend_service();
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_set_autostart(
    enabled: bool,
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    tokio::task::spawn_blocking(move || service::set_autostart_primary(enabled))
        .await
        .map_err(|error| WorkerError::msg(format!("autostart task failed: {error}")))??;
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_start_direct(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    require_ready().await?;
    let handle = (*direct).clone();
    tokio::task::spawn_blocking(move || service::start_direct(&handle))
        .await
        .map_err(|error| WorkerError::msg(format!("direct start failed: {error}")))??;
    watchdog.intend_direct();
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_stop_direct(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    watchdog.clear();
    let handle = (*direct).clone();
    tokio::task::spawn_blocking(move || service::stop_direct(&handle))
        .await
        .map_err(|error| WorkerError::msg(format!("direct stop failed: {error}")))??;
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_restart_direct(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    require_ready().await?;
    let handle = (*direct).clone();
    tokio::task::spawn_blocking(move || service::restart_direct(&handle))
        .await
        .map_err(|error| WorkerError::msg(format!("direct restart failed: {error}")))??;
    watchdog.intend_direct();
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_enable_linger(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    tokio::task::spawn_blocking(service::enable_linger)
        .await
        .map_err(|error| WorkerError::msg(format!("linger task failed: {error}")))??;
    snapshot(&direct, &watchdog).await
}

#[tauri::command]
pub async fn worker_reset_identity(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<WorkerSnapshot, WorkerError> {
    require_ready().await?;
    let handle = (*direct).clone();
    let (note, mode) = tokio::task::spawn_blocking(move || service::reset_identity(&handle))
        .await
        .map_err(|error| WorkerError::msg(format!("reset identity failed: {error}")))??;
    match mode {
        "direct" => watchdog.intend_direct(),
        "service" => watchdog.intend_service(),
        _ => watchdog.clear(),
    }
    let mut snap = snapshot(&direct, &watchdog).await?;
    if let Some(worker) = snap.worker.as_mut() {
        worker.runtime.watchdog_note = note;
    }
    Ok(snap)
}

#[tauri::command]
pub async fn worker_copy_diagnostics(
    direct: State<'_, DirectWorker>,
    watchdog: State<'_, Watchdog>,
) -> Result<String, WorkerError> {
    let snap = snapshot(&direct, &watchdog).await?;
    let logs = crate::logs::logs_read().unwrap_or(crate::logs::LogsSnapshot {
        folder: String::new(),
        sources: Vec::new(),
    });
    let secret = crate::auth::stored_api_key();
    Ok(crate::redact::redact_with_secret(
        &format_diagnostics(&snap, &logs),
        secret.as_deref(),
    ))
}

fn format_diagnostics(snap: &WorkerSnapshot, logs: &crate::logs::LogsSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {} diagnostics\n", crate::brand::DISPLAY_NAME));
    out.push_str(&format!("app_version: {}\n", crate::app_info::version()));
    out.push_str(&format!("config_path: {}\n", snap.config_path));
    match &snap.worker {
        None => out.push_str("worker: not configured\n"),
        Some(worker) => {
            let rt = &worker.runtime;
            out.push_str(&format!("worker_id: {}\n", worker.id));
            out.push_str(&format!("name: {}\n", worker.name));
            out.push_str(&format!("data_dir: {}\n", worker.data_dir));
            out.push_str(&format!("management_port: {}\n", worker.management_port));
            out.push_str(&format!("run_mode: {}\n", rt.run_mode));
            out.push_str(&format!("job_state: {}\n", rt.job_state));
            out.push_str(&format!("lifecycle: {}\n", rt.state));
            out.push_str(&format!("live_state: {}\n", rt.live_state));
            out.push_str(&format!("headline: {}\n", rt.headline));
            out.push_str(&format!("readyz_ok: {}\n", rt.readyz_ok));
            out.push_str(&format!("healthz_ok: {}\n", rt.healthz_ok));
            out.push_str(&format!("metrics_ok: {}\n", rt.metrics_ok));
            out.push_str(&format!("connected: {:?}\n", rt.connected));
            out.push_str(&format!("session_active: {:?}\n", rt.session_active));
            out.push_str(&format!("watchdog_intended: {}\n", rt.watchdog_intended));
            out.push_str(&format!("watchdog_restarts: {}\n", rt.watchdog_restarts));
            out.push_str(&format!("watchdog_note: {}\n", rt.watchdog_note));
            out.push_str(&format!("job_name: {}\n", rt.job_name));
            out.push_str(&format!("backend: {}\n", rt.backend));
            out.push_str("start_argv:");
            for arg in &worker.start_argv {
                out.push(' ');
                out.push_str(arg);
            }
            out.push('\n');
            if !worker.recent_agents.is_empty() {
                out.push_str("recent_agents:\n");
                for agent in &worker.recent_agents {
                    out.push_str(&format!("  {} {}\n", agent.bc_id, agent.claimed_at));
                }
            }
        }
    }
    out.push('\n');
    for source in &logs.sources {
        out.push_str(&format!("## {} ({})\n", source.name, source.path));
        if !source.exists {
            out.push_str("(not present)\n\n");
            continue;
        }
        if source.text.trim().is_empty() {
            out.push_str("(empty)\n\n");
            continue;
        }
        out.push_str(&source.text);
        out.push_str("\n\n");
    }
    out
}

async fn require_ready() -> Result<(), WorkerError> {
    let auth = crate::auth::build_status(false)
        .await
        .map_err(|error| WorkerError::msg(error.to_string()))?;
    if !auth.cli_present {
        return Err(WorkerError::msg("Install the official agent CLI first."));
    }
    if !auth.signed_in {
        return Err(WorkerError::msg(
            "Sign in before starting the personal worker.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_request_deserializes_camel_case() {
        let parsed: WorkerSaveRequest =
            serde_json::from_str(r#"{"name":"dev-laptop","workerDirs":["/home/dev/src/app"]}"#)
                .unwrap();
        assert_eq!(parsed.name, "dev-laptop");
        assert_eq!(parsed.worker_dirs, vec!["/home/dev/src/app"]);
    }

    #[test]
    fn busy_only_when_running_with_a_claim() {
        let idle = service::runtime_for(
            &WorkerRecord::defaults(String::new(), String::new(), Vec::new()),
            &DirectWorker::default(),
        );
        assert!(!worker_is_busy(&idle, true));
        let mut busy = idle.clone();
        busy.run_mode = "direct".into();
        busy.headline = "ready".into();
        assert!(worker_is_busy(&busy, true));
        assert!(!worker_is_busy(&busy, false));
        busy.session_active = Some(true);
        assert!(worker_is_busy(&busy, false));
        busy.session_active = Some(false);
        assert!(!worker_is_busy(&busy, true));
    }

    #[test]
    fn diagnostics_bundle_has_no_secret_placeholders_to_leak() {
        let snap = WorkerSnapshot {
            configured: false,
            extra_worker_count: 0,
            suggested_name: "dev".into(),
            config_path: "/tmp/config.toml".into(),
            worker: None,
        };
        let logs = crate::logs::LogsSnapshot {
            folder: String::new(),
            sources: vec![crate::logs::LogSourceView {
                name: "wrapper".into(),
                path: "/tmp/wrapper.log".into(),
                exists: true,
                text: "Authorization: Bearer eyJabc.def.ghi exact-secret-value".into(),
                truncated: false,
            }],
        };
        let text = crate::redact::redact_with_secret(
            &format_diagnostics(&snap, &logs),
            Some("exact-secret-value"),
        );
        assert!(text.contains("# worker-dock diagnostics"));
        assert!(!text.contains("eyJabc.def.ghi"));
        assert!(text.contains("Bearer ***"));
        assert!(!text.contains("exact-secret-value"));
    }

    #[test]
    fn view_serializes_start_argv_and_command_line() {
        let json = serde_json::json!({
            "id": "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60",
            "name": "my-devbox",
            "workerDirs": [],
            "managementPort": 18741,
            "idleReleaseTimeout": 0,
            "dataDir": "/tmp/data",
            "agentPath": "/tmp/agent",
            "startArgv": ["--disable-auto-update", "worker", "--name", "my-devbox", "start"],
            "commandLine": "/tmp/agent --disable-auto-update worker --name my-devbox start --verbose",
            "runtime": {
                "state": "stopped",
                "headline": "not_ready",
                "jobState": "missing",
                "backend": "systemd-user",
                "jobName": "",
                "installed": false,
                "enabled": false,
                "autostartEnabled": false,
                "linger": "disabled",
                "wrapperPath": "",
                "canEnableLinger": true,
                "restartRequired": false,
                "appliedName": null,
                "appliedWorkerDirs": [],
                "canStart": false,
                "canStop": false,
                "canRestart": false,
                "runMode": "none",
                "canStartDirect": false,
                "canStopDirect": false,
                "canRestartDirect": false,
                "autostartNote": "",
                "readyzOk": false,
                "healthzOk": false,
                "readyFromLog": false,
                "detail": ""
            }
        });
        let view: serde_json::Value = json;
        assert_eq!(view["startArgv"][1], "worker");
        assert_eq!(view["startArgv"][2], "--name");
        assert!(
            !view["commandLine"]
                .as_str()
                .unwrap()
                .contains("worker start --name")
        );
    }
}
