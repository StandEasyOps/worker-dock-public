use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde::Serialize;

use super::hosts::{PREFLIGHT_HOSTS, is_allowed_preflight_url, preflight_url};

const DNS_TIMEOUT: Duration = Duration::from_secs(5);
const TCP_TIMEOUT: Duration = Duration::from_secs(4);
const HTTP_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostProbe {
    pub host: String,
    pub role: String,
    pub class: String,
    pub why: String,
    pub ok: bool,
    pub stage: String,
    pub detail: String,
    pub status_code: Option<u16>,
}

pub fn proxy_is_set() -> bool {
    proxy_env_name().is_some()
}

/// Name of the proxy variable only. Never returns the URL (it may have userinfo).
pub fn proxy_env_name() -> Option<&'static str> {
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        if std::env::var(key)
            .ok()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Some(key);
        }
    }
    None
}

pub async fn probe_all() -> Vec<HostProbe> {
    let mut out = Vec::with_capacity(PREFLIGHT_HOSTS.len());
    for spec in PREFLIGHT_HOSTS {
        out.push(probe_one(spec).await);
    }
    out
}

async fn probe_one(spec: &super::hosts::HostSpec) -> HostProbe {
    let mut probe = HostProbe {
        host: spec.host.to_string(),
        role: spec.role.to_string(),
        class: spec.class.to_string(),
        why: spec.why.to_string(),
        ok: false,
        stage: "start".to_string(),
        detail: String::new(),
        status_code: None,
    };
    let url = match preflight_url(spec.host) {
        Ok(url) => url,
        Err(error) => {
            probe.stage = "allowlist".to_string();
            probe.detail = error;
            return probe;
        }
    };

    let host = spec.host.to_string();
    let dns = tokio::time::timeout(
        DNS_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            (host.as_str(), 443_u16).to_socket_addrs().map(|addrs| {
                addrs
                    .map(|addr| addr.to_string())
                    .take(4)
                    .collect::<Vec<_>>()
            })
        }),
    )
    .await;
    match dns {
        Ok(Ok(Ok(addrs))) if !addrs.is_empty() => {
            probe.stage = "dns".to_string();
            probe.detail = format!("resolved {}", addrs.join(", "));
        }
        Ok(Ok(Ok(_))) => {
            probe.stage = "dns".to_string();
            probe.detail = "DNS returned no addresses".to_string();
            if !proxy_is_set() {
                return probe;
            }
        }
        Ok(Ok(Err(error))) => {
            probe.stage = "dns".to_string();
            probe.detail = format!("DNS failed: {error}");
            if !proxy_is_set() {
                return probe;
            }
        }
        Ok(Err(error)) => {
            probe.stage = "dns".to_string();
            probe.detail = format!("DNS task failed: {error}");
            if !proxy_is_set() {
                return probe;
            }
        }
        Err(_) => {
            probe.stage = "dns".to_string();
            probe.detail = "DNS timed out".to_string();
            if !proxy_is_set() {
                return probe;
            }
        }
    }

    if !proxy_is_set() {
        let host = spec.host.to_string();
        let tcp = tokio::task::spawn_blocking(move || {
            let addrs = (host.as_str(), 443_u16).to_socket_addrs()?;
            let mut last = std::io::Error::other("no addresses");
            for addr in addrs {
                match TcpStream::connect_timeout(&addr, TCP_TIMEOUT) {
                    Ok(_) => return Ok(addr),
                    Err(error) => last = error,
                }
            }
            Err(last)
        })
        .await;
        match tcp {
            Ok(Ok(addr)) => {
                probe.stage = "tcp".to_string();
                probe.detail = format!("TCP 443 open on {addr}");
            }
            Ok(Err(error)) => {
                probe.stage = "tcp".to_string();
                probe.detail = format!("TCP 443 failed: {error}");
                return probe;
            }
            Err(error) => {
                probe.stage = "tcp".to_string();
                probe.detail = format!("TCP task failed: {error}");
                return probe;
            }
        }
    }

    if !is_allowed_preflight_url(&url) {
        probe.stage = "allowlist".to_string();
        probe.detail = "refusing to request a URL that left the allowlist".to_string();
        return probe;
    }
    let client = match reqwest::Client::builder()
        .user_agent("worker-dock/0.1.0 (Stand Easy Ops)")
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(HTTP_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            probe.stage = "tls".to_string();
            probe.detail = format!("HTTPS client: {error}");
            return probe;
        }
    };
    match client.get(url).send().await {
        Ok(response) => {
            probe.ok = true;
            probe.stage = "http".to_string();
            probe.status_code = Some(response.status().as_u16());
            let via = if proxy_is_set() { " via proxy" } else { "" };
            probe.detail = format!(
                "TLS+HTTPS reached (HTTP {}{via}). Reachable; HTTP status is not a start gate.",
                response.status().as_u16()
            );
        }
        Err(error) => {
            probe.stage = if error.is_timeout() {
                "timeout"
            } else if error.is_connect() {
                "tls"
            } else {
                "http"
            }
            .to_string();
            probe.detail = format!("HTTPS failed: {error}");
        }
    }
    probe
}

pub fn required_hosts_ok(probes: &[HostProbe]) -> bool {
    probes
        .iter()
        .filter(|probe| probe.class == "required")
        .all(|probe| probe.ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_artifact_does_not_fail_required() {
        let probes = vec![
            HostProbe {
                host: "api2.cursor.sh".into(),
                role: "session".into(),
                class: "required".into(),
                why: String::new(),
                ok: true,
                stage: "http".into(),
                detail: String::new(),
                status_code: Some(200),
            },
            HostProbe {
                host: "agent-stores.s3.us-east-1.amazonaws.com".into(),
                role: "artifact".into(),
                class: "artifact".into(),
                why: String::new(),
                ok: false,
                stage: "tls".into(),
                detail: "blocked".into(),
                status_code: None,
            },
        ];
        assert!(required_hosts_ok(&probes));
        let mut down = probes.clone();
        down[0].ok = false;
        assert!(!required_hosts_ok(&down));
    }

    #[test]
    fn dns_timeout_constant_is_bounded() {
        assert!(DNS_TIMEOUT <= Duration::from_secs(8));
    }
}
