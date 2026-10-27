//! In-app supervisor for a user-started worker.
//!
//! systemd `Restart=on-failure` still covers a service crash after the UI
//! closes. This watchdog also covers direct mode and an alive-but-stuck
//! process that fails localhost `/readyz` and `/metrics`.
//! It only restarts after the user started the worker (or the tray did).
//! Stop / Quit UI / uninstall clear the intent so we do not fight the user.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::service::{self, DirectWorker, RuntimeView};
use crate::worker::config::load_records;

const POLL: Duration = Duration::from_secs(3);
const START_GRACE: Duration = Duration::from_secs(20);
const UNHEALTHY_FOR: Duration = Duration::from_secs(12);
const BACKOFF: [u64; 5] = [2, 4, 8, 16, 30];
const MAX_RESTARTS: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intend {
    None,
    Direct,
    Service,
}

impl Intend {
    pub fn as_str(self) -> &'static str {
        match self {
            Intend::None => "none",
            Intend::Direct => "direct",
            Intend::Service => "service",
        }
    }
}

struct Inner {
    intend: Intend,
    last_start: Option<Instant>,
    pause_until: Option<Instant>,
    unhealthy_since: Option<Instant>,
    restart_count: u32,
    last_restart_unix: Option<u64>,
    backoff_index: usize,
    note: String,
    gave_up: bool,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            intend: Intend::None,
            last_start: None,
            pause_until: None,
            unhealthy_since: None,
            restart_count: 0,
            last_restart_unix: None,
            backoff_index: 0,
            note: String::new(),
            gave_up: false,
        }
    }
}

/// Shared watchdog intent and last visible note.
#[derive(Clone, Default)]
pub struct Watchdog {
    inner: Arc<Mutex<Inner>>,
}

impl Watchdog {
    pub fn intend_direct(&self) {
        self.intend(Intend::Direct);
    }

    pub fn intend_service(&self) {
        self.intend(Intend::Service);
    }

    fn intend(&self, mode: Intend) {
        let mut inner = lock(&self.inner);
        inner.intend = mode;
        inner.last_start = Some(Instant::now());
        inner.pause_until = Some(Instant::now() + START_GRACE);
        inner.unhealthy_since = None;
        inner.restart_count = 0;
        inner.backoff_index = 0;
        inner.gave_up = false;
        inner.note = String::new();
    }

    pub fn clear(&self) {
        let mut inner = lock(&self.inner);
        inner.intend = Intend::None;
        inner.pause_until = None;
        inner.unhealthy_since = None;
        inner.gave_up = false;
        inner.note = String::new();
    }

    pub fn intended(&self) -> Intend {
        lock(&self.inner).intend
    }

    pub fn apply_to(&self, runtime: &mut RuntimeView) {
        let inner = lock(&self.inner);
        runtime.watchdog_intended = inner.intend.as_str().to_string();
        runtime.watchdog_note = inner.note.clone();
        runtime.watchdog_restarts = inner.restart_count;
        runtime.watchdog_last_restart_unix = inner.last_restart_unix;
    }
}

/// Periodic check. Safe to call from the UI poll path or a background task.
pub async fn tick(direct: &DirectWorker, watchdog: &Watchdog) {
    let intend = watchdog.intended();
    if matches!(intend, Intend::None) {
        return;
    }
    if is_paused(watchdog) {
        return;
    }
    let Ok(records) = load_records() else {
        return;
    };
    let Some(record) = records.into_iter().next() else {
        return;
    };
    if uuid::Uuid::parse_str(&record.id).is_err() {
        return;
    }

    let mut runtime = service::runtime_for(&record, direct);
    service::status::enrich(&mut runtime, record.management_port, &record.id).await;
    let job_up = matches!(runtime.job_state.as_str(), "starting" | "running");
    let healthy = runtime.readyz_ok
        || runtime.metrics_ok
        || runtime.connected == Some(true)
        || runtime.ready_from_log;

    if job_up && healthy {
        mark_healthy(watchdog);
        return;
    }

    let reason = if !job_up {
        "Worker process exited; watchdog is restarting it."
    } else if past_grace(watchdog) {
        mark_unhealthy(watchdog);
        if !unhealthy_long_enough(watchdog) {
            return;
        }
        "Worker stopped responding on localhost /readyz and /metrics; watchdog is restarting it."
    } else {
        return;
    };

    if !begin_restart(watchdog, reason) {
        return;
    }

    let result = match intend {
        Intend::Direct => {
            if job_up {
                service::restart_direct(direct)
            } else {
                service::start_direct(direct)
            }
        }
        Intend::Service => {
            if job_up {
                service::restart_primary(direct)
            } else {
                service::start_primary(direct)
            }
        }
        Intend::None => Ok(()),
    };

    match result {
        Ok(()) => finish_restart_ok(watchdog),
        Err(error) => finish_restart_err(watchdog, &error.to_string()),
    }
}

pub fn poll_interval() -> Duration {
    POLL
}

fn is_paused(watchdog: &Watchdog) -> bool {
    let inner = lock(&watchdog.inner);
    if inner.gave_up {
        return true;
    }
    inner
        .pause_until
        .is_some_and(|until| Instant::now() < until)
}

fn past_grace(watchdog: &Watchdog) -> bool {
    let inner = lock(&watchdog.inner);
    inner
        .last_start
        .is_none_or(|started| started.elapsed() >= START_GRACE)
}

fn mark_healthy(watchdog: &Watchdog) {
    let mut inner = lock(&watchdog.inner);
    inner.unhealthy_since = None;
    inner.backoff_index = 0;
}

fn mark_unhealthy(watchdog: &Watchdog) {
    let mut inner = lock(&watchdog.inner);
    if inner.unhealthy_since.is_none() {
        inner.unhealthy_since = Some(Instant::now());
    }
}

fn unhealthy_long_enough(watchdog: &Watchdog) -> bool {
    let inner = lock(&watchdog.inner);
    inner
        .unhealthy_since
        .is_some_and(|since| since.elapsed() >= UNHEALTHY_FOR)
}

fn begin_restart(watchdog: &Watchdog, reason: &str) -> bool {
    let mut inner = lock(&watchdog.inner);
    if inner.gave_up || matches!(inner.intend, Intend::None) {
        return false;
    }
    if inner.restart_count >= MAX_RESTARTS {
        inner.gave_up = true;
        inner.note = format!(
            "Watchdog stopped after {MAX_RESTARTS} restarts. Start the worker again from Home."
        );
        return false;
    }
    inner.note = reason.to_string();
    true
}

fn finish_restart_ok(watchdog: &Watchdog) {
    let mut inner = lock(&watchdog.inner);
    inner.restart_count = inner.restart_count.saturating_add(1);
    inner.last_restart_unix = unix_now();
    inner.last_start = Some(Instant::now());
    inner.unhealthy_since = None;
    let wait = BACKOFF[inner.backoff_index.min(BACKOFF.len() - 1)];
    inner.pause_until = Some(Instant::now() + Duration::from_secs(wait) + START_GRACE);
    if inner.backoff_index + 1 < BACKOFF.len() {
        inner.backoff_index += 1;
    }
    inner.note = format!(
        "Watchdog restarted the worker ({}). Next backoff {}s if it fails again.",
        inner.restart_count, wait
    );
}

fn finish_restart_err(watchdog: &Watchdog, error: &str) {
    let mut inner = lock(&watchdog.inner);
    inner.restart_count = inner.restart_count.saturating_add(1);
    inner.last_restart_unix = unix_now();
    let wait = BACKOFF[inner.backoff_index.min(BACKOFF.len() - 1)];
    inner.pause_until = Some(Instant::now() + Duration::from_secs(wait));
    if inner.backoff_index + 1 < BACKOFF.len() {
        inner.backoff_index += 1;
    }
    inner.note = format!("Watchdog restart failed: {error}");
    if inner.restart_count >= MAX_RESTARTS {
        inner.gave_up = true;
        inner.note = format!(
            "Watchdog stopped after {MAX_RESTARTS} restarts. Start the worker again from Home."
        );
    }
}

fn unix_now() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn lock(inner: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_start_then_stop_clears_intent() {
        let wd = Watchdog::default();
        wd.intend_direct();
        assert_eq!(wd.intended(), Intend::Direct);
        wd.clear();
        assert_eq!(wd.intended(), Intend::None);
        assert!(!begin_restart(&wd, "no"));
    }

    #[test]
    fn gives_up_after_max_restarts() {
        let wd = Watchdog::default();
        wd.intend_service();
        {
            let mut inner = lock(&wd.inner);
            inner.restart_count = MAX_RESTARTS;
        }
        assert!(!begin_restart(&wd, "again"));
        let mut runtime = crate::service::runtime_for(
            &crate::worker::config::WorkerRecord::defaults(
                String::new(),
                String::new(),
                Vec::new(),
            ),
            &DirectWorker::default(),
        );
        wd.apply_to(&mut runtime);
        assert!(runtime.watchdog_note.contains("stopped after"));
        assert_eq!(runtime.watchdog_intended, "service");
        assert_eq!(runtime.watchdog_restarts, MAX_RESTARTS);
    }

    #[test]
    fn backoff_steps_are_monotonic() {
        let mut prev = 0;
        for step in BACKOFF {
            assert!(step >= prev);
            prev = step;
        }
        assert_eq!(BACKOFF[0], 2);
        assert_eq!(*BACKOFF.last().unwrap(), 30);
    }

    #[test]
    fn finish_restart_writes_a_visible_note() {
        let wd = Watchdog::default();
        wd.intend_direct();
        assert!(begin_restart(&wd, "Worker process exited; watchdog is restarting it."));
        finish_restart_ok(&wd);
        let mut runtime = crate::service::runtime_for(
            &crate::worker::config::WorkerRecord::defaults(
                String::new(),
                String::new(),
                Vec::new(),
            ),
            &DirectWorker::default(),
        );
        wd.apply_to(&mut runtime);
        assert!(runtime.watchdog_note.contains("Watchdog restarted"));
        assert_eq!(runtime.watchdog_restarts, 1);
        assert!(runtime.watchdog_last_restart_unix.is_some());
    }
}
