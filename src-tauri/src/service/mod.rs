//! Per-user supervisor for the one named personal worker.
//!
//! Linux: systemd user unit `worker-dock@<id>.service`.
//! Windows: interactive-user Task Scheduler task. WinSW is not used.
//! Closing the worker-dock window does not stop an installed job.
//!
//! Direct mode runs the same `startArgv` as a child of this process and
//! stops when the app quits. Service mode and direct mode cannot both run.

mod applied;
mod command;
mod direct;
mod linux;
pub(crate) mod metrics;
pub(crate) mod status;
pub(crate) mod watchdog;
mod windows;
mod wrapper;

use serde::Serialize;

use crate::worker::config::{WorkerRecord, load_records, save_autostart};
use crate::worker::error::WorkerError;

pub use direct::DirectWorker;
pub use watchdog::Watchdog;
pub use wrapper::run_supervised_worker;

/// Rewrite a previously installed systemd user template after upgrade.
#[cfg(unix)]
pub fn refresh_linux_unit_if_installed() -> Result<bool, WorkerError> {
    linux::refresh_installed_unit()
}

#[cfg(windows)]
const WINDOWS_AUTOSTART_NOTE: &str = "Windows starts the task at user logon with an interactive token, not at boot without a signed-in session, and not after logout. Linger is Linux-only.";
#[cfg(not(windows))]
const LINUX_AUTOSTART_NOTE: &str = "The systemd user unit starts at login when enabled. Start-at-boot without an open session needs linger (loginctl enable-linger).";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeView {
    /// Lifecycle after status enrich: stopped | starting | stopping | connected | error.
    pub state: String,
    /// ready | not_ready | starting | stopping. Auto-start is a separate field.
    pub headline: String,
    /// OS job only: stopped | starting | running | failed | missing | unknown.
    pub job_state: String,
    /// none | service | direct
    pub run_mode: String,
    pub backend: String,
    pub job_name: String,
    pub installed: bool,
    pub enabled: bool,
    pub autostart_enabled: bool,
    pub linger: String,
    pub wrapper_path: String,
    pub can_enable_linger: bool,
    pub restart_required: bool,
    pub applied_name: Option<String>,
    pub applied_worker_dirs: Vec<String>,
    pub can_start: bool,
    pub can_stop: bool,
    pub can_restart: bool,
    pub can_start_direct: bool,
    pub can_stop_direct: bool,
    pub can_restart_direct: bool,
    pub autostart_note: String,
    pub readyz_ok: bool,
    pub healthz_ok: bool,
    pub ready_from_log: bool,
    pub metrics_ok: bool,
    pub connected: Option<bool>,
    pub session_active: Option<bool>,
    pub last_activity_unix: Option<i64>,
    pub live_state: String,
    pub watchdog_note: String,
    pub watchdog_restarts: u32,
    pub watchdog_intended: String,
    pub watchdog_last_restart_unix: Option<u64>,
    pub detail: String,
}

pub fn runtime_for(record: &WorkerRecord, direct: &DirectWorker) -> RuntimeView {
    let wrapper_path = wrapper::wrapper_install_path()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let applied = applied::read(&record.id);
    let direct_snap = direct.snapshot();
    let service_state = service_state(record);
    let service_active = matches!(service_state, "running" | "starting");
    let (process_state, run_mode, backend) = if direct_snap.is_active() {
        (direct_snap.state(), "direct", "direct")
    } else if service_active {
        (service_state, "service", os_backend())
    } else {
        (service_state, "none", os_backend())
    };
    let headline = direct::headline_for(process_state);
    let restart_required = applied::restart_required(record, process_state, applied.as_ref());
    let direct_active = direct_snap.is_active();
    #[cfg(windows)]
    let (job_name, installed, enabled, autostart_enabled, linger, can_enable_linger, autostart_note) = {
        let job_name = windows::task_name(&record.id).unwrap_or_default();
        let query = windows::query(&record.id);
        let installed = !matches!(query, Ok(windows::TaskState::Missing) | Err(_));
        let autostart_enabled = installed && windows::logon_trigger_enabled(&record.id);
        (
            job_name,
            installed,
            installed && autostart_enabled,
            autostart_enabled,
            "not_applicable".to_string(),
            false,
            WINDOWS_AUTOSTART_NOTE.to_string(),
        )
    };
    #[cfg(not(windows))]
    let (job_name, installed, enabled, autostart_enabled, linger, can_enable_linger, autostart_note) = {
        let job_name = linux::unit_name(&record.id).unwrap_or_default();
        let linger = match linux::linger_state() {
            linux::LingerState::Enabled => "enabled",
            linux::LingerState::Disabled => "disabled",
            linux::LingerState::Unknown => "unknown",
        };
        let installed = linux::is_installed();
        let enabled = installed && linux::is_enabled(&record.id);
        (
            job_name,
            installed,
            enabled,
            enabled,
            linger.to_string(),
            linger != "enabled",
            LINUX_AUTOSTART_NOTE.to_string(),
        )
    };
    RuntimeView {
        state: status::lifecycle_from(process_state, false, false).to_string(),
        headline: headline.to_string(),
        job_state: if direct_active {
            "running".to_string()
        } else {
            service_state.to_string()
        },
        run_mode: run_mode.to_string(),
        backend: backend.to_string(),
        job_name,
        installed,
        enabled,
        autostart_enabled,
        linger,
        wrapper_path,
        can_enable_linger,
        restart_required,
        applied_name: applied.as_ref().map(|item| item.name.clone()),
        applied_worker_dirs: applied.map(|item| item.worker_dirs).unwrap_or_default(),
        can_start: installed && !direct_active && !service_active,
        can_stop: service_active && !direct_active,
        can_restart: installed
            && (service_active || matches!(service_state, "failed"))
            && !direct_active,
        can_start_direct: !direct_active && !service_active,
        can_stop_direct: direct_active,
        can_restart_direct: direct_active,
        autostart_note,
        readyz_ok: false,
        healthz_ok: false,
        ready_from_log: false,
        metrics_ok: false,
        connected: None,
        session_active: None,
        last_activity_unix: None,
        live_state: "offline".to_string(),
        watchdog_note: String::new(),
        watchdog_restarts: 0,
        watchdog_intended: "none".to_string(),
        watchdog_last_restart_unix: None,
        detail: String::new(),
    }
}

fn os_backend() -> &'static str {
    if cfg!(windows) {
        "scheduled-task"
    } else {
        "systemd-user"
    }
}

fn service_state(record: &WorkerRecord) -> &'static str {
    #[cfg(windows)]
    {
        match windows::query(&record.id) {
            Ok(windows::TaskState::Running) => "running",
            Ok(windows::TaskState::Failed) => "failed",
            Ok(windows::TaskState::Ready) => "stopped",
            Ok(windows::TaskState::Missing) => "missing",
            Err(_) => "unknown",
        }
    }
    #[cfg(not(windows))]
    {
        match linux::is_active(&record.id) {
            Ok(linux::UnitState::Active) => "running",
            Ok(linux::UnitState::Starting) => "starting",
            Ok(linux::UnitState::Failed) => "failed",
            Ok(linux::UnitState::Inactive) if linux::is_installed() => "stopped",
            Ok(linux::UnitState::Missing) | Ok(linux::UnitState::Inactive) => "missing",
            Ok(linux::UnitState::Unknown) | Err(_) => "unknown",
        }
    }
}

pub fn service_is_active(record: &WorkerRecord) -> bool {
    matches!(service_state(record), "running" | "starting")
}

pub fn install_primary(direct: &DirectWorker) -> Result<(), WorkerError> {
    direct.ensure_idle_for_service()?;
    let record = primary_record()?;
    let agent = wrapper::resolve_agent_sync()?;
    let plan = command::build_launch_plan(&record, &agent)?;
    wrapper::install_wrapper()?;
    wrapper::ensure_data_dir(&record.id)?;
    #[cfg(windows)]
    {
        if !windows::git_bash_present() {
            return Err(WorkerError::msg(windows::git_bash_required_message()));
        }
        windows::install_task(&record.id, &plan.work_dir, record.autostart)?;
    }
    #[cfg(not(windows))]
    {
        linux::install_unit()?;
        if record.autostart {
            linux::set_enabled(&record.id, true)?;
            let _ = linux::enable_linger();
        }
        let _ = plan;
    }
    Ok(())
}

pub fn uninstall_primary() -> Result<(), WorkerError> {
    let record = load_records()?
        .into_iter()
        .next()
        .ok_or_else(|| WorkerError::msg("Save the personal worker first."))?;
    if record.id.is_empty() {
        return Err(WorkerError::msg("Save the personal worker first."));
    }
    #[cfg(windows)]
    {
        windows::uninstall(&record.id)?;
    }
    #[cfg(not(windows))]
    {
        linux::uninstall(&record.id)?;
    }
    Ok(())
}

pub fn restart_primary(direct: &DirectWorker) -> Result<(), WorkerError> {
    stop_primary(direct)?;
    start_primary(direct)
}

pub fn start_primary(direct: &DirectWorker) -> Result<(), WorkerError> {
    direct.ensure_idle_for_service()?;
    let record = primary_record()?;
    let agent = wrapper::resolve_agent_sync()?;
    let plan = command::build_launch_plan(&record, &agent)?;
    wrapper::install_wrapper()?;
    wrapper::ensure_data_dir(&record.id)?;
    applied::write_from_args(&record.id, &plan.args)?;
    #[cfg(windows)]
    {
        if !windows::git_bash_present() {
            return Err(WorkerError::msg(windows::git_bash_required_message()));
        }
        windows::start(&record.id, &plan.work_dir, record.autostart)?;
    }
    #[cfg(not(windows))]
    {
        if record.autostart {
            linux::set_enabled(&record.id, true)?;
            let _ = linux::enable_linger();
        }
        linux::start(&record.id)?;
    }
    Ok(())
}

pub fn stop_primary(direct: &DirectWorker) -> Result<(), WorkerError> {
    if direct.is_active() {
        return Err(WorkerError::msg(
            "The worker is running in this window. Use Stop worker (without service).",
        ));
    }
    let record = primary_record()?;
    #[cfg(windows)]
    {
        windows::stop(&record.id)?;
    }
    #[cfg(not(windows))]
    {
        linux::stop(&record.id)?;
    }
    Ok(())
}

pub fn start_direct(direct: &DirectWorker) -> Result<(), WorkerError> {
    let record = primary_record()?;
    #[cfg(windows)]
    {
        if !windows::git_bash_present() {
            return Err(WorkerError::msg(windows::git_bash_required_message()));
        }
    }
    wrapper::ensure_data_dir(&record.id)?;
    direct.start(&record, service_is_active(&record))
}

pub fn stop_direct(direct: &DirectWorker) -> Result<(), WorkerError> {
    direct.stop()
}

pub fn restart_direct(direct: &DirectWorker) -> Result<(), WorkerError> {
    let record = primary_record()?;
    #[cfg(windows)]
    {
        if !windows::git_bash_present() {
            return Err(WorkerError::msg(windows::git_bash_required_message()));
        }
    }
    wrapper::ensure_data_dir(&record.id)?;
    direct.restart(&record, service_is_active(&record))
}

pub fn set_autostart_primary(enabled: bool) -> Result<(), WorkerError> {
    let record = save_autostart(enabled)?;
    let agent = wrapper::resolve_agent_sync().ok();
    let work_dir = record.worker_dirs.first().map(std::path::PathBuf::from);
    wrapper::install_wrapper()?;
    #[cfg(windows)]
    {
        let work = work_dir.ok_or_else(|| {
            WorkerError::msg("Add at least one worker folder before enabling autostart.")
        })?;
        let _ = agent;
        windows::set_enabled(&record.id, &work, enabled)?;
    }
    #[cfg(not(windows))]
    {
        let _ = (agent, work_dir);
        linux::set_enabled(&record.id, enabled)?;
        if enabled {
            let _ = linux::enable_linger();
        }
    }
    Ok(())
}

/// Windows Git Bash check for the Preflight screen. `None` on Linux.
pub fn windows_git_bash() -> Option<(bool, String)> {
    #[cfg(windows)]
    {
        Some((
            windows::git_bash_present(),
            windows::git_bash_required_message().to_string(),
        ))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn enable_linger() -> Result<(), WorkerError> {
    #[cfg(windows)]
    {
        Err(WorkerError::msg(
            "Linger is a Linux systemd-user setting. Windows uses a logon scheduled task and cannot start at boot without a signed-in session.",
        ))
    }
    #[cfg(not(windows))]
    {
        linux::enable_linger()
    }
}

/// Stop whatever is running, wipe the isolated `--data-dir`, then start again
/// with the same `--name` / `--worker-dir`. The official CLI keeps the logical
/// worker identity in that data dir; there is no `agent worker reset`.
pub fn reset_identity(direct: &DirectWorker) -> Result<(String, &'static str), WorkerError> {
    let record = primary_record()?;
    let was_direct = direct.is_active();
    let was_service = service_is_active(&record);
    if was_direct {
        stop_direct(direct)?;
    } else if was_service {
        stop_primary(direct)?;
    }
    crate::worker::paths::wipe_isolated_data_dir(&record.id)?;
    wrapper::ensure_data_dir(&record.id)?;
    if was_direct {
        start_direct(direct)?;
        return Ok((
            "Cleared the isolated worker data dir and started again without the service."
                .to_string(),
            "direct",
        ));
    }
    if was_service {
        start_primary(direct)?;
        return Ok((
            "Cleared the isolated worker data dir and started the service again.".to_string(),
            "service",
        ));
    }
    Ok((
        "Cleared the isolated worker data dir. Start the worker when you want a new identity."
            .to_string(),
        "none",
    ))
}

fn primary_record() -> Result<WorkerRecord, WorkerError> {
    let record = load_records()?
        .into_iter()
        .next()
        .ok_or_else(|| WorkerError::msg("Save the personal worker first."))?;
    if record.id.is_empty() || record.name.is_empty() || record.worker_dirs.is_empty() {
        return Err(WorkerError::msg(
            "Save a name and at least one folder first.",
        ));
    }
    Ok(record)
}
