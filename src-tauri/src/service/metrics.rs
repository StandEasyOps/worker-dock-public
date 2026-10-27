//! Parse the worker's documented localhost `GET /metrics` Prometheus text.
//!
//! Official My Machines / Team Pools gauges (names only; labels ignored):
//! `cursor_self_hosted_worker_connected`,
//! `cursor_self_hosted_worker_session_active`,
//! `cursor_self_hosted_worker_last_activity_unix_seconds`.
//! No other endpoints and no Cursor dashboard scraping.

/// Documented gauge: 1 when the worker is connected to Cursor.
pub const CONNECTED: &str = "cursor_self_hosted_worker_connected";
/// Documented gauge: 1 while an agent session is active on this worker.
pub const SESSION_ACTIVE: &str = "cursor_self_hosted_worker_session_active";
/// Documented gauge: unix seconds of last worker activity.
pub const LAST_ACTIVITY: &str = "cursor_self_hosted_worker_last_activity_unix_seconds";

const MAX_METRICS_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkerMetrics {
    pub connected: Option<bool>,
    pub session_active: Option<bool>,
    pub last_activity_unix: Option<i64>,
}

impl WorkerMetrics {
    pub fn empty() -> Self {
        Self {
            connected: None,
            session_active: None,
            last_activity_unix: None,
        }
    }
}

/// Truncate an oversized scrape and parse the documented gauges.
pub fn parse_metrics_text(text: &str) -> WorkerMetrics {
    let clipped = clip_metrics_text(text);
    let mut metrics = WorkerMetrics::empty();
    for line in clipped.lines() {
        let Some((name, value)) = parse_gauge_line(line) else {
            continue;
        };
        match name {
            CONNECTED => metrics.connected = Some(value >= 0.5),
            SESSION_ACTIVE => metrics.session_active = Some(value >= 0.5),
            LAST_ACTIVITY => {
                if value.is_finite() && value >= 0.0 {
                    metrics.last_activity_unix = Some(value as i64);
                }
            }
            _ => {}
        }
    }
    metrics
}

/// Live chip from OS job + documented probes. Never invents an API.
pub fn live_state_from(
    job_up: bool,
    lifecycle: &str,
    connected: Option<bool>,
    session_active: Option<bool>,
    readyz_ok: bool,
) -> &'static str {
    if !job_up {
        return if lifecycle == "error" {
            "error"
        } else if lifecycle == "stopping" {
            "stopping"
        } else {
            "offline"
        };
    }
    if lifecycle == "stopping" {
        return "stopping";
    }
    if session_active == Some(true) {
        return "busy";
    }
    if connected == Some(true) || readyz_ok {
        return if session_active == Some(false) {
            "idle"
        } else {
            "live"
        };
    }
    if lifecycle == "starting" {
        return "starting";
    }
    "unresponsive"
}

fn clip_metrics_text(text: &str) -> &str {
    if text.len() <= MAX_METRICS_BYTES {
        return text;
    }
    let mut end = MAX_METRICS_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn parse_gauge_line(line: &str) -> Option<(&str, f64)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (name, rest) = if let Some(brace) = line.find('{') {
        let close = line[brace + 1..].find('}')?;
        let name = line[..brace].trim();
        let rest = line[brace + 1 + close + 1..].trim();
        (name, rest)
    } else {
        let mut parts = line.splitn(2, |c: char| c.is_whitespace());
        (parts.next()?.trim(), parts.next()?.trim())
    };
    if name.is_empty() || !is_metric_name(name) {
        return None;
    }
    let token = rest.split_whitespace().next()?;
    if !token.bytes().all(|b| {
        b.is_ascii_digit()
            || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E')
    }) {
        return None;
    }
    let value: f64 = token.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some((name, value))
}

fn is_metric_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == ':') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_gauges() {
        let text = "\
# HELP cursor_self_hosted_worker_connected Whether this worker is connected\n\
# TYPE cursor_self_hosted_worker_connected gauge\n\
cursor_self_hosted_worker_connected 1\n\
cursor_self_hosted_worker_session_active{reason=\"ok\"} 0\n\
cursor_self_hosted_worker_last_activity_unix_seconds 1.726e9\n\
cursor_self_hosted_worker_connect_success_total 4\n";
        let parsed = parse_metrics_text(text);
        assert_eq!(parsed.connected, Some(true));
        assert_eq!(parsed.session_active, Some(false));
        assert_eq!(parsed.last_activity_unix, Some(1_726_000_000));
    }

    #[test]
    fn treats_half_or_more_as_true() {
        let parsed = parse_metrics_text(
            "cursor_self_hosted_worker_connected 0\n\
             cursor_self_hosted_worker_session_active 1.0\n",
        );
        assert_eq!(parsed.connected, Some(false));
        assert_eq!(parsed.session_active, Some(true));
    }

    #[test]
    fn ignores_comments_and_unknown_names() {
        let parsed = parse_metrics_text("# TYPE foo gauge\nfoo 1\nnot a line");
        assert_eq!(parsed, WorkerMetrics::empty());
    }

    #[test]
    fn rejects_non_numeric_and_non_finite() {
        let parsed = parse_metrics_text(
            "cursor_self_hosted_worker_connected NaN\n\
             cursor_self_hosted_worker_session_active +Inf\n\
             cursor_self_hosted_worker_last_activity_unix_seconds not-a-number\n",
        );
        assert_eq!(parsed, WorkerMetrics::empty());
    }

    #[test]
    fn clips_on_a_char_boundary_when_a_multibyte_char_crosses_the_cap() {
        let head = "cursor_self_hosted_worker_connected 1\n";
        let mut text = head.to_string();
        let pad = MAX_METRICS_BYTES - text.len() - 1;
        text.push_str(&"x".repeat(pad));
        text.push('€');
        text.push_str("cursor_self_hosted_worker_session_active 1\n");
        assert!(text.len() > MAX_METRICS_BYTES);
        assert!(!text.is_char_boundary(MAX_METRICS_BYTES));
        let parsed = parse_metrics_text(&text);
        assert_eq!(parsed.connected, Some(true));
        assert_eq!(parsed.session_active, None);
    }

    #[test]
    fn live_state_uses_metrics_and_readyz() {
        assert_eq!(
            live_state_from(false, "stopped", None, None, false),
            "offline"
        );
        assert_eq!(
            live_state_from(false, "error", None, None, false),
            "error"
        );
        assert_eq!(
            live_state_from(true, "starting", None, None, false),
            "starting"
        );
        assert_eq!(
            live_state_from(true, "connected", Some(true), Some(false), true),
            "idle"
        );
        assert_eq!(
            live_state_from(true, "connected", Some(true), Some(true), true),
            "busy"
        );
        assert_eq!(
            live_state_from(true, "connected", None, None, true),
            "live"
        );
        assert_eq!(
            live_state_from(true, "connected", Some(false), Some(false), false),
            "unresponsive"
        );
        assert_eq!(
            live_state_from(true, "stopping", Some(true), Some(true), true),
            "stopping"
        );
    }
}
