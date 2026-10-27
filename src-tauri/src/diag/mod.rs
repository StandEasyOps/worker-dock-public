//! Network preflight and `agent worker debug --json`.
//!
//! This is not CI packaging and does not call `GET /v0/private-workers`.

mod debug;
mod error;
mod hosts;
mod preflight;

use serde::Serialize;

use debug::{DebugView, run_debug};
use error::DiagError;
use preflight::{HostProbe, probe_all, proxy_env_name, required_hosts_ok};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagSnapshot {
    pub cli_present: bool,
    pub signed_in: bool,
    pub proxy: Option<String>,
    pub can_start_network: bool,
    pub summary: String,
    pub hosts: Vec<HostProbe>,
    pub git_bash: Option<CheckView>,
    pub debug: DebugView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckView {
    pub name: String,
    pub ok: bool,
    pub required: bool,
    pub detail: String,
}

#[tauri::command]
pub async fn diag_run() -> Result<DiagSnapshot, DiagError> {
    let auth = crate::auth::build_status(false)
        .await
        .map_err(|error| DiagError::msg(error.to_string()))?;
    let hosts = probe_all().await;
    let git_bash = crate::service::windows_git_bash().map(|(ok, detail)| CheckView {
        name: "Git Bash".to_string(),
        ok,
        required: true,
        detail,
    });
    let network_ok = required_hosts_ok(&hosts);
    let git_ok = git_bash.as_ref().is_none_or(|check| check.ok);
    let can_start_network = network_ok && git_ok;
    let debug = run_debug().await;
    let proxy = proxy_env_name()
        .map(|name| format!("{name} is set. HTTPS checks honor the proxy. The URL is not shown."));
    let summary = summary_text(
        can_start_network,
        network_ok,
        git_bash.as_ref(),
        &hosts,
        &debug,
    );
    Ok(DiagSnapshot {
        cli_present: auth.cli_present,
        signed_in: auth.signed_in,
        proxy,
        can_start_network,
        summary,
        hosts,
        git_bash,
        debug,
    })
}

fn summary_text(
    can_start_network: bool,
    network_ok: bool,
    git_bash: Option<&CheckView>,
    hosts: &[HostProbe],
    debug: &DebugView,
) -> String {
    let mut parts = Vec::new();
    if can_start_network {
        parts.push("Required outbound hosts are reachable.".to_string());
    } else if !network_ok {
        parts.push("A required outbound host failed DNS, TCP, or TLS.".to_string());
    }
    if let Some(check) = git_bash {
        if !check.ok {
            parts.push(check.detail.clone());
        }
    }
    let blocked_artifacts: Vec<_> = hosts
        .iter()
        .filter(|probe| probe.class == "artifact" && !probe.ok)
        .map(|probe| probe.host.as_str())
        .collect();
    if !blocked_artifacts.is_empty() {
        parts.push(format!(
            "Artifact host(s) failed ({}). Uploads may fail; the worker can still start.",
            blocked_artifacts.join(", ")
        ));
    }
    if let Some(reason) = &debug.skipped {
        parts.push(format!("Worker debug was not run: {reason}"));
    } else if debug.ok {
        parts.push("Official worker debug returned JSON.".to_string());
    } else if debug.ran {
        parts.push("Official worker debug did not succeed. See the report below.".to_string());
    }
    parts.push(
        "Corporate TLS inspection: set NODE_EXTRA_CA_CERTS (and network.useHttp1ForAgent if needed) for the official CLI. worker-dock preflight uses rustls, not the OS trust store."
            .to_string(),
    );
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_does_not_treat_artifact_as_start_failure() {
        let hosts = vec![HostProbe {
            host: "agent-stores.s3.us-east-1.amazonaws.com".into(),
            role: "artifact".into(),
            class: "artifact".into(),
            why: String::new(),
            ok: false,
            stage: "tls".into(),
            detail: "blocked".into(),
            status_code: None,
        }];
        let debug = DebugView {
            ran: false,
            ok: false,
            skipped: Some("Save the personal worker first.".into()),
            exit_code: None,
            auth_method: None,
            worker_id: None,
            visibility: None,
            computer_use: None,
            desktop_share: None,
            warnings: Vec::new(),
            text: String::new(),
            json: None,
        };
        let text = summary_text(true, true, None, &hosts, &debug);
        assert!(text.contains("Artifact"));
        assert!(text.contains("can still start"));
        assert!(!text.contains("Required outbound host failed"));
    }
}
