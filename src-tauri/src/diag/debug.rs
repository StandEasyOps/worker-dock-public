use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tokio::process::Command;

use crate::cli::detect::{DISABLE_AUTO_UPDATE, detect};
use crate::cli::error::redact_secret;
use crate::process::{hide_tokio_window, prefer_hidden_agent, wait_tracked_output};
use crate::worker::config::{WorkerRecord, load_records};
use crate::worker::validate::validate_worker_dirs;

const DEBUG_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugView {
    pub ran: bool,
    pub ok: bool,
    pub skipped: Option<String>,
    pub exit_code: Option<i32>,
    pub auth_method: Option<String>,
    pub worker_id: Option<String>,
    pub visibility: Option<String>,
    pub computer_use: Option<String>,
    pub desktop_share: Option<String>,
    pub warnings: Vec<String>,
    pub text: String,
    pub json: Option<String>,
}

pub fn debug_args(
    record: &WorkerRecord,
    auto_update_flag_supported: bool,
) -> Result<Vec<String>, String> {
    if record.name.trim().is_empty() {
        return Err("Save a display name before running worker debug.".into());
    }
    let dirs = validate_worker_dirs(&record.worker_dirs).map_err(|error| error.to_string())?;
    let mut args = Vec::new();
    if auto_update_flag_supported {
        args.push(DISABLE_AUTO_UPDATE.to_string());
    }
    args.push("worker".to_string());
    args.push("--name".to_string());
    args.push(record.name.clone());
    for dir in dirs {
        args.push("--worker-dir".to_string());
        args.push(dir);
    }
    args.push("debug".to_string());
    args.push("--json".to_string());
    let joined = args.join(" ");
    if joined.contains("--pool")
        || joined.contains("--api-key")
        || joined.contains("mint-github")
        || joined.contains("controller")
    {
        return Err("refusing to construct a pool or argv-secret debug command".into());
    }
    Ok(args)
}

pub async fn run_debug() -> DebugView {
    let config = match crate::cli::config::load() {
        Ok(config) => config,
        Err(error) => return skipped(error.to_string()),
    };
    let Some(detected) = (match detect(&config).await {
        Ok(found) => found,
        Err(error) => return skipped(error.to_string()),
    }) else {
        return skipped("Install the official agent CLI first.".into());
    };
    let record = match primary_record() {
        Ok(record) => record,
        Err(error) => return skipped(error),
    };
    let args = match debug_args(&record, detected.auto_update_flag_supported) {
        Ok(args) => args,
        Err(error) => return skipped(error),
    };
    let api_key = match crate::auth::api_key_for_spawn() {
        Ok(key) => key,
        Err(error) => return skipped(error.to_string()),
    };

    let agent = prefer_hidden_agent(&detected.path);
    let mut command = Command::new(&agent);
    command
        .args(&args)
        .env_remove("CURSOR_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    hide_tokio_window(&mut command);
    if let Some(key) = api_key.as_deref().filter(|value| !value.is_empty()) {
        command.env("CURSOR_API_KEY", key);
    }
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return DebugView {
                ran: true,
                ok: false,
                skipped: None,
                exit_code: None,
                auth_method: None,
                worker_id: None,
                visibility: None,
                computer_use: None,
                desktop_share: None,
                warnings: Vec::new(),
                text: format!("failed to spawn official CLI: {error}"),
                json: None,
            };
        }
    };
    let output = match wait_tracked_output(child, DEBUG_TIMEOUT).await {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            return DebugView {
                ran: true,
                ok: false,
                skipped: None,
                exit_code: None,
                auth_method: None,
                worker_id: None,
                visibility: None,
                computer_use: None,
                desktop_share: None,
                warnings: Vec::new(),
                text: "worker debug timed out after 90 seconds.".to_string(),
                json: None,
            };
        }
        Err(error) => {
            return DebugView {
                ran: true,
                ok: false,
                skipped: None,
                exit_code: None,
                auth_method: None,
                worker_id: None,
                visibility: None,
                computer_use: None,
                desktop_share: None,
                warnings: Vec::new(),
                text: format!("worker debug failed: {error}"),
                json: None,
            };
        }
    };
    let secret = api_key.as_deref();
    let stdout = redact_secret(String::from_utf8_lossy(&output.stdout).trim(), secret);
    let stderr = redact_secret(String::from_utf8_lossy(&output.stderr).trim(), secret);
    let combined = if stderr.is_empty() {
        stdout.clone()
    } else if stdout.is_empty() {
        stderr.clone()
    } else {
        format!("{stdout}\n{stderr}")
    };
    parse_debug_output(&combined, output.status.code().unwrap_or(-1))
}

pub fn parse_debug_output(text: &str, exit_code: i32) -> DebugView {
    let Some(value) = extract_json(text) else {
        return DebugView {
            ran: true,
            ok: false,
            skipped: None,
            exit_code: Some(exit_code),
            auth_method: None,
            worker_id: None,
            visibility: None,
            computer_use: None,
            desktop_share: None,
            warnings: Vec::new(),
            text: if text.trim().is_empty() {
                format!("worker debug exited {exit_code} with no output.")
            } else {
                text.to_string()
            },
            json: None,
        };
    };
    DebugView {
        ran: true,
        ok: exit_code == 0,
        skipped: None,
        exit_code: Some(exit_code),
        auth_method: first_string(&value, &["auth", "method"])
            .or_else(|| first_string(&value, &["auth"])),
        worker_id: first_string(&value, &["worker", "workerId"])
            .or_else(|| first_string(&value, &["workerId"])),
        visibility: visibility_summary(&value),
        computer_use: summarize_value(value.get("computerUse")),
        desktop_share: summarize_value(value.get("desktopShare")),
        warnings: string_list(&value, "warnings"),
        text: String::new(),
        json: Some(serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())),
    }
}

fn skipped(reason: String) -> DebugView {
    DebugView {
        ran: false,
        ok: false,
        skipped: Some(reason),
        exit_code: None,
        auth_method: None,
        worker_id: None,
        visibility: None,
        computer_use: None,
        desktop_share: None,
        warnings: Vec::new(),
        text: String::new(),
        json: None,
    }
}

fn primary_record() -> Result<WorkerRecord, String> {
    let record = load_records()
        .map_err(|error| error.to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "Save the personal worker first.".to_string())?;
    if record.name.is_empty() || record.worker_dirs.is_empty() {
        return Err("Save a name and at least one folder first.".into());
    }
    Ok(record)
}

fn extract_json(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    serde_json::from_str(&text[start..]).ok()
}

fn first_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    match current {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn string_list(value: &Value, key: &str) -> Vec<String> {
    match value.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .filter(|item| !item.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn visibility_summary(value: &Value) -> Option<String> {
    let probe = value.get("visibilityProbe")?;
    let non_privacy = probe.get("nonPrivacy").and_then(Value::as_bool);
    let privacy = probe.get("privacy").and_then(Value::as_bool);
    Some(format!(
        "non-privacy={}, privacy={}",
        bool_label(non_privacy),
        bool_label(privacy)
    ))
}

fn bool_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn summarize_value(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Null) | None => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Bool(flag)) => Some(flag.to_string()),
        Some(other) => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::config::WorkerRecord;

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
    fn debug_argv_matches_design() {
        let args = debug_args(&record(), true).unwrap();
        let joined = args.join(" ");
        assert_eq!(args[0], DISABLE_AUTO_UPDATE);
        assert_eq!(args[1], "worker");
        assert!(joined.contains("--name dev-laptop"));
        assert!(joined.contains("--worker-dir"));
        assert!(joined.ends_with("debug --json"));
        assert!(!joined.contains("--pool"));
        assert!(!joined.contains("--api-key"));
        assert!(!joined.contains("start"));
    }

    #[test]
    fn parses_formatter_field_names() {
        let text = r#"{
          "auth": {"method": "session"},
          "bridge": {"ghostMode": false, "bridgeType": "direct"},
          "worker": {"workerId": "w-1"},
          "computerUse": {"supported": false},
          "desktopShare": {"supported": false},
          "visibilityProbe": {"nonPrivacy": true, "privacy": false, "sampleWorkers": []},
          "warnings": ["example"]
        }"#;
        let view = parse_debug_output(text, 0);
        assert!(view.ok);
        assert_eq!(view.auth_method.as_deref(), Some("session"));
        assert_eq!(view.worker_id.as_deref(), Some("w-1"));
        assert_eq!(
            view.visibility.as_deref(),
            Some("non-privacy=yes, privacy=no")
        );
        assert_eq!(view.warnings, vec!["example"]);
        assert!(view.json.is_some());
    }

    #[test]
    fn unauthenticated_text_is_not_json() {
        let text = "Authentication required for worker debug. Please run 'agent login', or provide an API key with --api-key or CURSOR_API_KEY.";
        let view = parse_debug_output(text, 1);
        assert!(!view.ok);
        assert!(view.json.is_none());
        assert!(view.text.contains("Authentication required"));
        assert!(!view.text.contains("sk-"));
    }
}
