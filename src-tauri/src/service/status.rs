//! Localhost management probes for the Connected chip and live/idle/busy.
//!
//! This is worker status, not network preflight. Only `127.0.0.1` is contacted.
//! Documented paths: `/readyz`, `/healthz`, `/metrics`.
//! `GET /v0/private-workers` and `agent worker debug --json` are out of scope.

use std::time::Duration;

use url::Url;

use crate::logs;
use crate::service::metrics::{self, WorkerMetrics};
use crate::service::RuntimeView;
use crate::worker::paths::{WRAPPER_LOG, worker_data_dir};

pub const READY_LOG_LINE: &str = "Private worker started";
const PROBE_TIMEOUT: Duration = Duration::from_millis(800);
const METRICS_BODY_LIMIT: usize = 64 * 1024;

const ALLOWED_PROBE_PATHS: &[&str] = &["readyz", "healthz", "metrics"];

/// Build `http://127.0.0.1:<port>/<path>` and refuse anything else.
pub fn management_probe_url(port: u16, path: &str) -> Result<Url, String> {
    if port < 1024 {
        return Err("management port must be 1024 or higher".into());
    }
    if !ALLOWED_PROBE_PATHS.contains(&path) {
        return Err("probe path must be readyz, healthz, or metrics".into());
    }
    let parsed = Url::parse(&format!("http://127.0.0.1:{port}/{path}"))
        .map_err(|error| error.to_string())?;
    if !is_allowed_management_url(&parsed) {
        return Err(
            "refusing a management URL that is not http://127.0.0.1/readyz|/healthz|/metrics"
                .into(),
        );
    }
    Ok(parsed)
}

pub fn is_allowed_management_url(url: &Url) -> bool {
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && matches!(url.path(), "/readyz" | "/healthz" | "/metrics")
        && url.port().is_some_and(|port| port >= 1024)
}

pub fn log_says_ready(text: &str) -> bool {
    text.contains(READY_LOG_LINE)
}

/// Map OS job + localhost ready signals to DESIGN chips.
/// Ready-from-log is ignored unless the job is actually up (stale logs).
pub fn lifecycle_from(job_state: &str, readyz_ok: bool, ready_from_log: bool) -> &'static str {
    let up = matches!(job_state, "starting" | "running");
    let ready = up && (readyz_ok || ready_from_log);
    match job_state {
        "failed" => "error",
        "stopped" | "missing" => "stopped",
        "unknown" => "stopped",
        "stopping" => "stopping",
        "starting" | "running" if ready => "connected",
        "starting" | "running" => "starting",
        _ => "stopped",
    }
}

pub fn detail_for(
    job_state: &str,
    lifecycle: &str,
    readyz_ok: bool,
    ready_from_log: bool,
) -> String {
    match lifecycle {
        "connected" if readyz_ok => "Localhost /readyz returned 200.".to_string(),
        "connected" if ready_from_log => {
            format!("Log line: {READY_LOG_LINE}.")
        }
        "starting" => {
            "Process is up; waiting for localhost /readyz or /metrics.".to_string()
        }
        "stopping" => "Worker is stopping…".to_string(),
        "error" => "Supervisor reported a failed job.".to_string(),
        "stopped" if job_state == "unknown" => {
            "Supervisor state is unknown; treating the worker as stopped.".to_string()
        }
        _ => String::new(),
    }
}

pub async fn enrich(runtime: &mut RuntimeView, port: u16, id: &str) {
    let log_text = wrapper_tail(id);
    let ready_from_log = log_says_ready(&log_text);
    let should_probe = matches!(runtime.job_state.as_str(), "starting" | "running")
        || runtime.run_mode == "direct";
    let mut readyz_ok = false;
    let mut healthz_ok = false;
    let mut scraped = WorkerMetrics::empty();
    let mut metrics_ok = false;
    if should_probe {
        if let (Ok(ready_url), Ok(health_url), Ok(metrics_url)) = (
            management_probe_url(port, "readyz"),
            management_probe_url(port, "healthz"),
            management_probe_url(port, "metrics"),
        ) {
            readyz_ok = probe_ok(ready_url).await;
            healthz_ok = probe_ok(health_url).await;
            if let Some(body) = probe_text(metrics_url).await {
                scraped = metrics::parse_metrics_text(&body);
                metrics_ok = true;
            }
        }
    }
    runtime.readyz_ok = readyz_ok;
    runtime.healthz_ok = healthz_ok;
    runtime.metrics_ok = metrics_ok;
    runtime.connected = scraped.connected;
    runtime.session_active = scraped.session_active;
    runtime.last_activity_unix = scraped.last_activity_unix;
    runtime.ready_from_log =
        ready_from_log && matches!(runtime.job_state.as_str(), "starting" | "running");
    let ready_signal = readyz_ok || scraped.connected == Some(true);
    runtime.state =
        lifecycle_from(&runtime.job_state, ready_signal, runtime.ready_from_log).to_string();
    let job_up = matches!(runtime.job_state.as_str(), "starting" | "running");
    runtime.live_state = metrics::live_state_from(
        job_up,
        &runtime.state,
        scraped.connected,
        scraped.session_active,
        readyz_ok,
    )
    .to_string();
    if runtime.live_state == "unresponsive" {
        runtime.headline = "not_ready".to_string();
    } else if matches!(runtime.live_state.as_str(), "live" | "idle" | "busy") {
        runtime.headline = "ready".to_string();
    }
    runtime.detail = detail_for(
        &runtime.job_state,
        &runtime.state,
        readyz_ok,
        runtime.ready_from_log,
    );
    if runtime.live_state == "busy" {
        runtime.detail = "Localhost /metrics reports an active agent session.".to_string();
    } else if runtime.live_state == "idle" {
        runtime.detail = "Worker is live and idle (localhost /metrics).".to_string();
    } else if runtime.live_state == "unresponsive" {
        runtime.detail =
            "Process is up but localhost /readyz and /metrics are not responding.".to_string();
    }
}

fn wrapper_tail(id: &str) -> String {
    let Ok(dir) = worker_data_dir(id) else {
        return String::new();
    };
    logs::tail_existing(&dir.join(WRAPPER_LOG)).unwrap_or_default()
}

async fn probe_ok(url: Url) -> bool {
    match send_probe(url).await {
        Some(response) => response.status().is_success(),
        None => false,
    }
}

fn append_capped(buf: &mut Vec<u8>, chunk: &[u8], limit: usize) -> bool {
    let room = limit.saturating_sub(buf.len());
    if room == 0 {
        return true;
    }
    if chunk.len() > room {
        buf.extend_from_slice(&chunk[..room]);
        return true;
    }
    buf.extend_from_slice(chunk);
    buf.len() >= limit
}

async fn probe_text(url: Url) -> Option<String> {
    let mut response = send_probe(url).await?;
    if !response.status().is_success() {
        return None;
    }
    let mut buf = Vec::new();
    loop {
        if buf.len() >= METRICS_BODY_LIMIT {
            break;
        }
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if append_capped(&mut buf, &chunk, METRICS_BODY_LIMIT) {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => return None,
        }
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

async fn send_probe(url: Url) -> Option<reqwest::Response> {
    if !is_allowed_management_url(&url) {
        return None;
    }
    let client = reqwest::Client::builder()
        .user_agent(crate::app_info::user_agent())
        .timeout(PROBE_TIMEOUT)
        .connect_timeout(Duration::from_millis(400))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .ok()?;
    client.get(url).send().await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_body_read_stops_at_the_limit() {
        let mut buf = vec![b'x'; METRICS_BODY_LIMIT - 2];
        assert!(append_capped(&mut buf, b"abcdef", METRICS_BODY_LIMIT));
        assert_eq!(buf.len(), METRICS_BODY_LIMIT);
        assert_eq!(&buf[buf.len() - 2..], b"ab");
        assert!(append_capped(&mut buf, b"more", METRICS_BODY_LIMIT));
        assert_eq!(buf.len(), METRICS_BODY_LIMIT);
    }

    #[test]
    fn probe_url_is_loopback_http_only() {
        let ready = management_probe_url(18741, "readyz").unwrap();
        assert_eq!(ready.as_str(), "http://127.0.0.1:18741/readyz");
        assert!(is_allowed_management_url(&ready));
        assert!(management_probe_url(18741, "healthz").is_ok());
        let metrics = management_probe_url(18741, "metrics").unwrap();
        assert_eq!(metrics.as_str(), "http://127.0.0.1:18741/metrics");
        assert!(is_allowed_management_url(&metrics));
        assert!(management_probe_url(80, "readyz").is_err());
        assert!(management_probe_url(18741, "debug").is_err());
        assert!(management_probe_url(18741, "../readyz").is_err());
    }

    #[test]
    fn rejects_non_loopback_management_urls() {
        for raw in [
            "http://0.0.0.0:18741/readyz",
            "http://localhost:18741/readyz",
            "https://127.0.0.1:18741/readyz",
            "http://127.0.0.1:18741/metrics?x=1",
            "http://127.0.0.1:18741/debug",
            "http://127.0.0.1:80/readyz",
            "http://127.0.0.1:18741/readyz?x=1",
            "http://user@127.0.0.1:18741/readyz",
            "http://example.com:18741/readyz",
        ] {
            let url = Url::parse(raw).unwrap();
            assert!(!is_allowed_management_url(&url), "{raw}");
        }
    }

    #[test]
    fn ready_line_is_exact_phrase() {
        assert!(log_says_ready("INFO Private worker started"));
        assert!(log_says_ready("Private worker started on the bridge"));
        assert!(!log_says_ready("Starting worker..."));
        assert!(!log_says_ready(
            "ERROR Authentication required for worker mode"
        ));
    }

    #[test]
    fn lifecycle_uses_job_and_ready_signals() {
        assert_eq!(lifecycle_from("stopped", false, false), "stopped");
        assert_eq!(lifecycle_from("stopped", true, true), "stopped");
        assert_eq!(lifecycle_from("failed", true, true), "error");
        assert_eq!(lifecycle_from("unknown", false, false), "stopped");
        assert_eq!(lifecycle_from("running", false, false), "starting");
        assert_eq!(lifecycle_from("starting", false, false), "starting");
        assert_eq!(lifecycle_from("running", true, false), "connected");
        assert_eq!(lifecycle_from("starting", false, true), "connected");
        assert_eq!(lifecycle_from("running", false, true), "connected");
        assert_eq!(lifecycle_from("stopping", false, false), "stopping");
    }
}
