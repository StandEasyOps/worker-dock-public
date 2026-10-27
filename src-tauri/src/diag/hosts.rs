use url::Url;

/// Documented My Machines outbound hosts. Only these names are probed.
#[derive(Debug, Clone, Copy)]
pub struct HostSpec {
    pub host: &'static str,
    pub role: &'static str,
    pub class: &'static str,
    pub why: &'static str,
}

pub const PREFLIGHT_HOSTS: &[HostSpec] = &[
    HostSpec {
        host: "api2.cursor.sh",
        role: "session",
        class: "required",
        why: "Agent session / default official CLI endpoint",
    },
    HostSpec {
        host: "api2direct.cursor.sh",
        role: "bridge",
        class: "required",
        why: "Bridge / privacy routing",
    },
    HostSpec {
        host: "downloads.cursor.com",
        role: "downloads",
        class: "required",
        why: "Official CLI packages",
    },
    HostSpec {
        host: "agent-stores.s3.us-east-1.amazonaws.com",
        role: "artifact",
        class: "artifact",
        why: "Artifact upload (blocked does not stop the worker)",
    },
    HostSpec {
        host: "cloud-agent-artifacts.s3.us-east-1.amazonaws.com",
        role: "artifact",
        class: "artifact",
        why: "Artifact upload fallback (blocked does not stop the worker)",
    },
    HostSpec {
        host: "cursor.blob.core.windows.net",
        role: "workspace",
        class: "optional",
        why: "Live workspace server download (--connect-workspace only)",
    },
    HostSpec {
        host: "cursor.com",
        role: "login",
        class: "required",
        why: "Browser login (loginDeepControl) and the official install script",
    },
];

pub fn spec_for(host: &str) -> Option<&'static HostSpec> {
    PREFLIGHT_HOSTS.iter().find(|spec| spec.host == host)
}

/// `https://<allowlisted-host>/` only. No userinfo, port, path, or query.
pub fn preflight_url(host: &str) -> Result<Url, String> {
    if spec_for(host).is_none() {
        return Err("host is not on the preflight allowlist".into());
    }
    if host.is_empty()
        || host.contains('/')
        || host.contains('\\')
        || host.contains('@')
        || host.contains(':')
        || host.contains('?')
        || host.contains('#')
        || host.contains("..")
    {
        return Err("refusing a preflight host that is not a bare hostname".into());
    }
    let parsed = Url::parse(&format!("https://{host}/")).map_err(|error| error.to_string())?;
    if !is_allowed_preflight_url(&parsed) {
        return Err("refusing a preflight URL that is not https://<allowlisted-host>/".into());
    }
    Ok(parsed)
}

pub fn is_allowed_preflight_url(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    url.scheme() == "https"
        && spec_for(host).is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_builds_https_root_only() {
        let url = preflight_url("api2.cursor.sh").unwrap();
        assert_eq!(url.as_str(), "https://api2.cursor.sh/");
        assert!(is_allowed_preflight_url(&url));
        assert!(preflight_url("evil.example").is_err());
        assert!(preflight_url("api2.cursor.sh:443").is_err());
        assert!(preflight_url("api2.cursor.sh/login").is_err());
    }

    #[test]
    fn rejects_non_allowlisted_urls() {
        for raw in [
            "http://api2.cursor.sh/",
            "https://evil.example/",
            "https://user@api2.cursor.sh/",
            "https://api2.cursor.sh:8443/",
            "https://api2.cursor.sh/readyz",
            "https://api2.cursor.sh/?x=1",
            "https://127.0.0.1/",
        ] {
            let url = Url::parse(raw).unwrap();
            assert!(!is_allowed_preflight_url(&url), "{raw}");
        }
    }

    #[test]
    fn artifact_hosts_are_not_required() {
        let artifacts: Vec<_> = PREFLIGHT_HOSTS
            .iter()
            .filter(|spec| spec.class == "artifact")
            .map(|spec| spec.host)
            .collect();
        assert!(artifacts.contains(&"agent-stores.s3.us-east-1.amazonaws.com"));
        assert!(artifacts.contains(&"cloud-agent-artifacts.s3.us-east-1.amazonaws.com"));
        assert!(
            !PREFLIGHT_HOSTS
                .iter()
                .any(|spec| spec.host == "api2.cursor.sh" && spec.class == "artifact")
        );
    }
}
