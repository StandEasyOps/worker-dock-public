//! Allowlisted URLs opened in the default browser from the UI.

use url::Url;

pub const CURSOR_AGENTS_URL: &str = "https://cursor.com/agents";
pub const CURSOR_DOCS_URL: &str = "https://cursor.com/docs/cloud-agent/self-hosted/my-machines";
pub const STANDEASYOPS_URL: &str = "https://standeasyops.com";

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|error| error.to_string())?;
    if !is_allowlisted(&parsed) {
        return Err("That URL is not allowlisted.".into());
    }
    open::that_detached(parsed.as_str()).map_err(|error| error.to_string())
}

pub fn is_allowlisted(url: &Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if is_cursor_page(url) {
        return true;
    }
    if is_standeasyops_home(url) {
        return true;
    }
    is_local_readyz(url)
}

fn is_standeasyops_home(url: &Url) -> bool {
    let Ok(expected) = Url::parse(STANDEASYOPS_URL) else {
        return false;
    };
    url == &expected
}

fn is_cursor_page(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    if url.host_str() != Some("cursor.com") {
        return false;
    }
    if url.port().is_some() && url.port() != Some(443) {
        return false;
    }
    if url.query().is_some() || url.fragment().is_some() {
        return false;
    }
    if url.as_str() == CURSOR_DOCS_URL {
        return true;
    }
    if url.path() == "/agents" {
        return url.as_str() == CURSOR_AGENTS_URL;
    }
    let Some(bc_id) = url.path().strip_prefix("/agents/") else {
        return false;
    };
    crate::logs::claims::is_bc_id(bc_id)
}

fn is_local_readyz(url: &Url) -> bool {
    if url.scheme() != "http" {
        return false;
    }
    if url.host_str() != Some("127.0.0.1") {
        return false;
    }
    let Some(port) = url.port() else {
        return false;
    };
    if port == 0 {
        return false;
    }
    if url.query().is_some() || url.fragment().is_some() {
        return false;
    }
    url.path() == "/readyz"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Url {
        Url::parse(raw).expect("url")
    }

    #[test]
    fn allowlists_official_pages_and_localhost_readyz() {
        assert!(is_allowlisted(&parse(CURSOR_AGENTS_URL)));
        assert!(is_allowlisted(&parse(CURSOR_DOCS_URL)));
        assert!(is_allowlisted(&parse(STANDEASYOPS_URL)));
        assert!(is_allowlisted(&parse("https://standeasyops.com/")));
        assert!(is_allowlisted(&parse("http://127.0.0.1:18741/readyz")));
        assert!(is_allowlisted(&parse("http://127.0.0.1:9/readyz")));
        assert!(is_allowlisted(&parse(
            "https://cursor.com/agents/bc-00000000-0000-4000-8000-000000000001"
        )));
    }

    #[test]
    fn rejects_other_urls() {
        assert!(!is_allowlisted(&parse("https://cursor.com/agents?x=1")));
        assert!(!is_allowlisted(&parse(
            "https://cursor.com/agents/bc-not-a-uuid"
        )));
        assert!(!is_allowlisted(&parse(
            "https://cursor.com/agents/../secret"
        )));
        assert!(!is_allowlisted(&parse("https://evil.com/agents")));
        assert!(!is_allowlisted(&parse("https://cursor.com.evil/agents")));
        assert!(!is_allowlisted(&parse("https://www.standeasyops.com")));
        assert!(!is_allowlisted(&parse("https://standeasyops.com/about")));
        assert!(!is_allowlisted(&parse("http://standeasyops.com")));
        assert!(!is_allowlisted(&parse("https://standeasyops.com/?q=1")));
        assert!(!is_allowlisted(&parse("http://127.0.0.1:18741/")));
        assert!(!is_allowlisted(&parse("http://127.0.0.1:18741/metrics")));
        assert!(!is_allowlisted(&parse("http://localhost:18741/readyz")));
        assert!(!is_allowlisted(&parse("https://127.0.0.1:18741/readyz")));
        assert!(!is_allowlisted(&parse("http://127.0.0.1/readyz")));
        assert!(!is_allowlisted(&parse(
            "https://user:pass@cursor.com/agents"
        )));
    }
}
