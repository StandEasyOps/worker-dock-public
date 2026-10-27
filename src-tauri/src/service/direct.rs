use std::process::Child;
use std::sync::{Arc, Mutex};

use crate::service::command::build_launch_plan;
use crate::service::wrapper::{resolve_agent_sync, spawn_logged_agent};
use crate::worker::config::WorkerRecord;
use crate::worker::error::WorkerError;

const SERVICE_RUNNING_MSG: &str =
    "Stop the background service before starting the worker in this window.";
const DIRECT_RUNNING_MSG: &str = "Stop the in-app worker before starting the background service.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectPhase {
    Idle,
    Starting,
    Running,
    Stopping,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectSnapshot {
    pub phase: DirectPhase,
}

impl DirectSnapshot {
    pub fn state(&self) -> &'static str {
        match self.phase {
            DirectPhase::Idle => "stopped",
            DirectPhase::Starting => "starting",
            DirectPhase::Running => "running",
            DirectPhase::Stopping => "stopping",
        }
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.phase, DirectPhase::Idle)
    }
}

struct DirectInner {
    phase: DirectPhase,
    child: Option<Child>,
}

impl Default for DirectInner {
    fn default() -> Self {
        Self {
            phase: DirectPhase::Idle,
            child: None,
        }
    }
}

/// In-app `agent` child. Stops when the worker-dock process exits.
#[derive(Clone, Default)]
pub struct DirectWorker {
    inner: Arc<Mutex<DirectInner>>,
}

impl DirectWorker {
    pub fn snapshot(&self) -> DirectSnapshot {
        self.reap();
        let inner = lock(&self.inner);
        DirectSnapshot { phase: inner.phase }
    }

    pub fn is_active(&self) -> bool {
        self.snapshot().is_active()
    }

    pub fn ensure_idle_for_service(&self) -> Result<(), WorkerError> {
        refuse_exclusive(self.is_active(), false, false)
    }

    pub fn start(&self, record: &WorkerRecord, service_active: bool) -> Result<(), WorkerError> {
        self.reap();
        refuse_exclusive(self.is_active(), service_active, true)?;
        {
            let mut inner = lock(&self.inner);
            if inner.phase != DirectPhase::Idle {
                return Err(WorkerError::msg("The in-app worker is already running."));
            }
            inner.phase = DirectPhase::Starting;
        }
        match spawn_direct(record) {
            Ok(child) => {
                let mut inner = lock(&self.inner);
                inner.child = Some(child);
                inner.phase = DirectPhase::Running;
                Ok(())
            }
            Err(error) => {
                let mut inner = lock(&self.inner);
                inner.phase = DirectPhase::Idle;
                inner.child = None;
                Err(error)
            }
        }
    }

    pub fn stop(&self) -> Result<(), WorkerError> {
        self.reap();
        let mut inner = lock(&self.inner);
        if inner.phase == DirectPhase::Idle {
            return Ok(());
        }
        inner.phase = DirectPhase::Stopping;
        let result = terminate(inner.child.take());
        inner.phase = DirectPhase::Idle;
        result
    }

    pub fn restart(&self, record: &WorkerRecord, service_active: bool) -> Result<(), WorkerError> {
        self.stop()?;
        self.start(record, service_active)
    }

    pub fn stop_on_app_quit(&self) {
        let _ = self.stop();
    }

    fn reap(&self) {
        let mut inner = lock(&self.inner);
        let Some(child) = inner.child.as_mut() else {
            if inner.phase == DirectPhase::Running || inner.phase == DirectPhase::Starting {
                inner.phase = DirectPhase::Idle;
            }
            return;
        };
        match child.try_wait() {
            Ok(None) => {}
            Ok(Some(_)) | Err(_) => {
                inner.child = None;
                inner.phase = DirectPhase::Idle;
            }
        }
    }
}

fn spawn_direct(record: &WorkerRecord) -> Result<Child, WorkerError> {
    let agent = resolve_agent_sync()?;
    let plan = build_launch_plan(record, &agent)?;
    crate::service::applied::write_from_args(&record.id, &plan.args)?;
    spawn_logged_agent(&plan)
}

fn terminate(child: Option<Child>) -> Result<(), WorkerError> {
    let Some(mut child) = child else {
        return Ok(());
    };
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

fn lock(inner: &Mutex<DirectInner>) -> std::sync::MutexGuard<'_, DirectInner> {
    inner.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub(crate) fn refuse_exclusive(
    direct_active: bool,
    service_active: bool,
    want_direct: bool,
) -> Result<(), WorkerError> {
    if want_direct && service_active {
        return Err(WorkerError::msg(SERVICE_RUNNING_MSG));
    }
    if !want_direct && direct_active {
        return Err(WorkerError::msg(DIRECT_RUNNING_MSG));
    }
    Ok(())
}

/// Headline for the Home lamp. Auto-start is a separate field.
/// Ready = running. Not ready = not running. Amber only starting/stopping.
pub fn headline_for(state: &str) -> &'static str {
    match state {
        "starting" => "starting",
        "stopping" => "stopping",
        "running" | "connected" => "ready",
        _ => "not_ready",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headline_ready_only_when_running() {
        assert_eq!(headline_for("running"), "ready");
        assert_eq!(headline_for("connected"), "ready");
        assert_eq!(headline_for("stopped"), "not_ready");
        assert_eq!(headline_for("failed"), "not_ready");
        assert_eq!(headline_for("unknown"), "not_ready");
        assert_eq!(headline_for("starting"), "starting");
        assert_eq!(headline_for("stopping"), "stopping");
    }

    #[test]
    fn exclusive_modes_cannot_overlap() {
        assert!(refuse_exclusive(true, false, false).is_err());
        assert!(refuse_exclusive(false, true, true).is_err());
        assert!(refuse_exclusive(false, false, true).is_ok());
        assert!(refuse_exclusive(false, false, false).is_ok());
    }

    #[test]
    fn idle_direct_snapshot_is_stopped() {
        let worker = DirectWorker::default();
        let snap = worker.snapshot();
        assert_eq!(snap.phase, DirectPhase::Idle);
        assert_eq!(snap.state(), "stopped");
        assert!(!snap.is_active());
        assert_eq!(headline_for(snap.state()), "not_ready");
    }

    #[test]
    fn stop_on_quit_is_idle_when_nothing_running() {
        let worker = DirectWorker::default();
        worker.stop_on_app_quit();
        assert!(!worker.is_active());
    }

    #[cfg(unix)]
    #[test]
    fn start_helper_child_then_quit_stops_it() {
        use std::process::Command;
        let worker = DirectWorker::default();
        {
            let mut inner = lock(&worker.inner);
            inner.phase = DirectPhase::Starting;
        }
        let child = Command::new("sleep")
            .arg("30")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        {
            let mut inner = lock(&worker.inner);
            inner.child = Some(child);
            inner.phase = DirectPhase::Running;
        }
        assert!(worker.is_active());
        assert_eq!(headline_for(worker.snapshot().state()), "ready");
        worker.stop_on_app_quit();
        assert!(!worker.is_active());
        assert_eq!(headline_for(worker.snapshot().state()), "not_ready");
    }
}
