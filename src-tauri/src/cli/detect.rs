use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;
use tokio::sync::Mutex as AsyncMutex;

use crate::cli::config::{CliConfig, configured_binary};
use crate::cli::error::CliError;
use crate::cli::paths::{default_search_paths, is_usable_file, look_on_path};
use crate::process::{hide_tokio_window, prefer_hidden_agent, wait_tracked_output};

/// `--version` is probed once and reused until the binary disappears or
/// install/update invalidates the cache. Prevents the 2/sec console spam.
static DETECTED: Mutex<Option<DetectedCli>> = Mutex::new(None);
static DETECT_FLIGHT: OnceLock<AsyncMutex<()>> = OnceLock::new();

pub(crate) const DISABLE_AUTO_UPDATE: &str = "--disable-auto-update";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedCli {
    pub path: PathBuf,
    pub version: String,
    pub auto_update_flag_supported: bool,
}

#[derive(Debug, Deserialize)]
pub struct AboutJson {
    #[serde(rename = "cliVersion")]
    #[allow(dead_code)]
    pub cli_version: Option<String>,
    #[serde(rename = "latestStatus")]
    #[allow(dead_code)]
    pub latest_status: Option<String>,
    #[serde(rename = "latestVersion")]
    pub latest_version: Option<String>,
    #[serde(rename = "userEmail")]
    pub user_email: Option<String>,
}

pub fn invalidate_detect_cache() {
    if let Ok(mut guard) = DETECTED.lock() {
        *guard = None;
    }
}

pub fn remember_detected(value: Option<DetectedCli>) {
    if let Ok(mut guard) = DETECTED.lock() {
        *guard = value;
    }
}

fn cached_if_usable() -> Option<DetectedCli> {
    let Ok(guard) = DETECTED.lock() else {
        return None;
    };
    guard
        .as_ref()
        .filter(|found| is_usable_file(&found.path))
        .cloned()
}

/// Locate the official CLI. `--version` runs at most once while the path
/// still exists; overlapping callers wait on the same in-flight probe.
pub async fn detect(config: &CliConfig) -> Result<Option<DetectedCli>, CliError> {
    if let Some(cached) = cached_if_usable() {
        return Ok(Some(cached));
    }
    let _flight = DETECT_FLIGHT
        .get_or_init(|| AsyncMutex::new(()))
        .lock()
        .await;
    if let Some(cached) = cached_if_usable() {
        return Ok(Some(cached));
    }
    let found = detect_fresh(config).await?;
    remember_detected(found.clone());
    Ok(found)
}

async fn detect_fresh(config: &CliConfig) -> Result<Option<DetectedCli>, CliError> {
    let mut candidates = Vec::new();
    if let Some(configured) = configured_binary(config) {
        candidates.push(prefer_hidden_agent(configured));
    }
    candidates.extend(default_search_paths()?);
    if let Some(from_path) = look_on_path("agent") {
        candidates.push(prefer_hidden_agent(&from_path));
    }
    let mut seen = std::collections::HashSet::new();
    for candidate in candidates {
        let candidate = prefer_hidden_agent(&candidate);
        if !seen.insert(candidate.clone()) {
            continue;
        }
        if !is_usable_file(&candidate) {
            continue;
        }
        match probe(&candidate).await {
            Ok(detected) => return Ok(Some(detected)),
            Err(CliError::CommandFailed { .. }) => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

pub async fn probe(path: &Path) -> Result<DetectedCli, CliError> {
    let with_flag = run_agent(path, &[DISABLE_AUTO_UPDATE, "--version"]).await;
    match with_flag {
        Ok(output) => {
            let version = parse_version_output(&output)?;
            Ok(DetectedCli {
                path: path.to_path_buf(),
                version,
                auto_update_flag_supported: true,
            })
        }
        Err(CliError::CommandFailed { stderr, .. })
            if stderr.to_ascii_lowercase().contains("unknown option") =>
        {
            let output = run_agent(path, &["--version"]).await?;
            let version = parse_version_output(&output)?;
            Ok(DetectedCli {
                path: path.to_path_buf(),
                version,
                auto_update_flag_supported: false,
            })
        }
        Err(error) => Err(error),
    }
}

pub async fn about(path: &Path, auto_update_flag_supported: bool) -> Result<AboutJson, CliError> {
    about_with_env(path, auto_update_flag_supported, None).await
}

pub async fn about_with_env(
    path: &Path,
    auto_update_flag_supported: bool,
    api_key: Option<&str>,
) -> Result<AboutJson, CliError> {
    let args = with_auto_update_flag(auto_update_flag_supported, &["about", "--format", "json"]);
    let output = run_agent_with_env(path, &args, api_key).await?;
    serde_json::from_str(&output)
        .map_err(|error| CliError::msg(format!("agent about --format json: {error}")))
}

pub fn with_auto_update_flag(supported: bool, args: &[&'static str]) -> Vec<&'static str> {
    let mut out = Vec::with_capacity(args.len() + 1);
    if supported {
        out.push(DISABLE_AUTO_UPDATE);
    }
    out.extend_from_slice(args);
    out
}

pub async fn run_agent(path: &Path, args: &[&str]) -> Result<String, CliError> {
    run_agent_with_env(path, args, None).await
}

/// Spawn `agent` with `--disable-auto-update` already in `args` when supported.
///
/// `CURSOR_API_KEY` is removed from the child environment unless `api_key` is
/// set. The key is never placed on argv. Do not `Debug`-format `command` after
/// the env is set — std Debug can include environment values.
pub async fn run_agent_with_env(
    path: &Path,
    args: &[&str],
    api_key: Option<&str>,
) -> Result<String, CliError> {
    let path = prefer_hidden_agent(path);
    let mut command = Command::new(&path);
    command
        .args(args)
        .env_remove("CURSOR_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    hide_tokio_window(&mut command);
    if let Some(key) = api_key.filter(|value| !value.is_empty()) {
        command.env("CURSOR_API_KEY", key);
    }
    let child = command.spawn()?;
    let result = wait_tracked_output(child, Duration::from_secs(30))
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::TimedOut {
                CliError::msg(format!("timed out running {}", path.display()))
            } else {
                CliError::from(error)
            }
        })?;
    let stdout =
        crate::cli::error::redact_secret(String::from_utf8_lossy(&result.stdout).trim(), api_key);
    let stderr =
        crate::cli::error::redact_secret(String::from_utf8_lossy(&result.stderr).trim(), api_key);
    if !result.status.success() {
        return Err(CliError::CommandFailed {
            path: path.to_path_buf(),
            status: result.status.code().unwrap_or(-1),
            stderr: if stderr.is_empty() { stdout } else { stderr },
        });
    }
    Ok(stdout)
}

fn parse_version_output(output: &str) -> Result<String, CliError> {
    let first = output.lines().next().unwrap_or(output).trim();
    if let Ok(parsed) = crate::cli::version::CliVersion::parse(first) {
        return Ok(parsed.as_str().to_string());
    }
    if let Some(found) = crate::cli::version::version_from_installer_text(output) {
        return Ok(found);
    }
    Err(CliError::msg(format!(
        "could not parse agent --version output: {output}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_version_line() {
        assert_eq!(
            parse_version_output("2026.10.19-6bbf2c4\n").unwrap(),
            "2026.10.19-6bbf2c4"
        );
    }

    #[test]
    fn parses_about_json() {
        let parsed: AboutJson = serde_json::from_str(
            r#"{
              "cliVersion": "2026.10.19-6bbf2c4",
              "latestStatus": "up_to_date",
              "latestVersion": "2026.10.19-6bbf2c4",
              "userEmail": "dev@example.com"
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.cli_version.as_deref(), Some("2026.10.19-6bbf2c4"));
        assert_eq!(parsed.latest_status.as_deref(), Some("up_to_date"));
        assert_eq!(parsed.user_email.as_deref(), Some("dev@example.com"));
    }

    #[test]
    fn auto_update_flag_prefixes_when_supported() {
        assert_eq!(
            with_auto_update_flag(true, &["status", "--format", "json"]),
            vec![DISABLE_AUTO_UPDATE, "status", "--format", "json"]
        );
        assert_eq!(with_auto_update_flag(false, &["logout"]), vec!["logout"]);
    }

    #[test]
    fn detect_cache_round_trips_until_invalidated() {
        invalidate_detect_cache();
        remember_detected(Some(DetectedCli {
            path: PathBuf::from("/tmp/missing-agent-cache-test"),
            version: "2026.10.19-6bbf2c4".into(),
            auto_update_flag_supported: true,
        }));
        // Path is not a usable file, so the cache must not be reused.
        assert!(cached_if_usable().is_none());
        invalidate_detect_cache();
        assert!(cached_if_usable().is_none());
    }
}
