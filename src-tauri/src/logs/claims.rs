//! Claimed Cloud Agent ids from `<data-dir>/wrapper.log`.
//!
//! Missing or rotated files must not fail the UI. Only `bc-<uuid>` values
//! from `Worker claimed by background composer` lines are kept.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;

use crate::worker::paths::WRAPPER_LOG;

const CLAIM_MARKER: &str = "Worker claimed by background composer";
const MAX_RECENT: usize = 5;
const SCAN_BYTES: u64 = 256 * 1024;
const ROTATED_LOG: &str = "wrapper.log.1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentAgent {
    pub bc_id: String,
    pub claimed_at: String,
    pub url: String,
}

pub fn recent_from_data_dir(data_dir: &str) -> Vec<RecentAgent> {
    if data_dir.trim().is_empty() {
        return Vec::new();
    }
    let root = PathBuf::from(data_dir);
    let mut claims = Vec::new();
    collect_claims(&root.join(ROTATED_LOG), &mut claims);
    collect_claims(&root.join(WRAPPER_LOG), &mut claims);
    newest_unique(claims)
}

pub fn is_bc_id(value: &str) -> bool {
    let Some(uuid) = value.strip_prefix("bc-") else {
        return false;
    };
    Uuid::parse_str(uuid).is_ok()
}

pub fn agent_chat_url(bc_id: &str) -> Option<String> {
    if !is_bc_id(bc_id) {
        return None;
    }
    Some(format!("https://cursor.com/agents/{bc_id}"))
}

pub fn parse_claim_line(line: &str) -> Option<RecentAgent> {
    if !line.contains(CLAIM_MARKER) {
        return None;
    }
    let bc_id = extract_bc_id(line)?;
    let url = agent_chat_url(&bc_id)?;
    Some(RecentAgent {
        bc_id,
        claimed_at: extract_time(line),
        url,
    })
}

fn collect_claims(path: &Path, out: &mut Vec<RecentAgent>) {
    let Ok(text) = read_tail_lossy(path) else {
        return;
    };
    for line in text.lines() {
        if let Some(claim) = parse_claim_line(line) {
            out.push(claim);
        }
    }
}

fn newest_unique(claims: Vec<RecentAgent>) -> Vec<RecentAgent> {
    let mut seen = std::collections::HashSet::new();
    let mut recent = Vec::new();
    for claim in claims.into_iter().rev() {
        if !seen.insert(claim.bc_id.clone()) {
            continue;
        }
        recent.push(claim);
        if recent.len() == MAX_RECENT {
            break;
        }
    }
    recent
}

fn extract_bc_id(line: &str) -> Option<String> {
    let key = line.find("bcId")?;
    let after = line.get(key..)?;
    let start = after.find("bc-")?;
    let rest = after.get(start..)?;
    let end = rest
        .find(|c: char| !(c.is_ascii_hexdigit() || c == '-' || c == 'b' || c == 'c'))
        .unwrap_or(rest.len());
    let candidate = rest.get(..end)?.to_string();
    if is_bc_id(&candidate) {
        Some(candidate)
    } else {
        None
    }
}

fn extract_time(line: &str) -> String {
    let token = line.split_whitespace().next().unwrap_or("");
    if looks_like_time(token) {
        token.trim_end_matches([',', ';']).to_string()
    } else {
        String::new()
    }
}

fn looks_like_time(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() >= 19
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && (bytes[10] == b'T' || bytes[10] == b' ')
        && bytes[13] == b':'
        && bytes[16] == b':'
}

fn read_tail_lossy(path: &Path) -> Result<String, ()> {
    let meta = std::fs::metadata(path).map_err(|_| ())?;
    if !meta.is_file() {
        return Err(());
    }
    let mut file = File::open(path).map_err(|_| ())?;
    if meta.len() > SCAN_BYTES {
        file.seek(SeekFrom::End(-(SCAN_BYTES as i64)))
            .map_err(|_| ())?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| ())?;
    if meta.len() > SCAN_BYTES {
        if let Some(idx) = bytes.iter().position(|byte| *byte == b'\n') {
            bytes = bytes[idx + 1..].to_vec();
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SAMPLE: &str = r#"INFO  Worker claimed by background composer ctx=worker-mode meta={workerId: "pw_1", bcId: "bc-00000000-0000-4000-8000-000000000001", other: 1}"#;

    #[test]
    fn parses_claim_line_without_timestamp() {
        let claim = parse_claim_line(SAMPLE).unwrap();
        assert_eq!(claim.bc_id, "bc-00000000-0000-4000-8000-000000000001");
        assert_eq!(
            claim.url,
            "https://cursor.com/agents/bc-00000000-0000-4000-8000-000000000001"
        );
        assert!(claim.claimed_at.is_empty());
    }

    #[test]
    fn parses_timestamp_and_ignores_junk() {
        let line = "2026-10-21T13:42:07.123Z INFO Worker claimed by background composer meta={bcId: \"bc-aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee\"}";
        let claim = parse_claim_line(line).unwrap();
        assert_eq!(claim.claimed_at, "2026-10-21T13:42:07.123Z");
        assert!(
            parse_claim_line(
                "INFO something else bcId: \"bc-aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee\""
            )
            .is_none()
        );
        assert!(
            parse_claim_line("Worker claimed by background composer bcId: \"not-a-bc\"").is_none()
        );
    }

    #[test]
    fn missing_log_is_empty() {
        assert!(recent_from_data_dir("/tmp/worker-dock-no-such-dir").is_empty());
        assert!(recent_from_data_dir("").is_empty());
    }

    #[test]
    fn newest_unique_from_current_and_rotated() {
        let dir = tempfile::tempdir().unwrap();
        let older = dir.path().join(ROTATED_LOG);
        let current = dir.path().join(WRAPPER_LOG);
        let mut rotated = File::create(&older).unwrap();
        writeln!(
            rotated,
            "2026-10-20T10:00:00Z INFO Worker claimed by background composer meta={{bcId: \"bc-11111111-1111-4111-8111-111111111111\"}}"
        )
        .unwrap();
        writeln!(
            rotated,
            "2026-10-20T11:00:00Z INFO Worker claimed by background composer meta={{bcId: \"bc-22222222-2222-4222-8222-222222222222\"}}"
        )
        .unwrap();
        let mut live = File::create(&current).unwrap();
        writeln!(
            live,
            "2026-10-21T09:00:00Z INFO Worker claimed by background composer meta={{bcId: \"bc-22222222-2222-4222-8222-222222222222\"}}"
        )
        .unwrap();
        writeln!(
            live,
            "2026-10-21T10:00:00Z INFO Worker claimed by background composer meta={{bcId: \"bc-00000000-0000-4000-8000-000000000001\"}}"
        )
        .unwrap();
        let recent = recent_from_data_dir(&dir.path().display().to_string());
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].bc_id, "bc-00000000-0000-4000-8000-000000000001");
        assert_eq!(recent[1].bc_id, "bc-22222222-2222-4222-8222-222222222222");
        assert_eq!(recent[1].claimed_at, "2026-10-21T09:00:00Z");
        assert_eq!(recent[2].bc_id, "bc-11111111-1111-4111-8111-111111111111");
    }

    #[test]
    fn caps_at_five_newest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(WRAPPER_LOG);
        let mut live = File::create(&path).unwrap();
        for index in 1..=6 {
            writeln!(
                live,
                "2026-10-21T1{index}:00:00Z INFO Worker claimed by background composer meta={{bcId: \"bc-{index:08}-1111-4111-8111-111111111111\"}}"
            )
            .unwrap();
        }
        let recent = recent_from_data_dir(&dir.path().display().to_string());
        assert_eq!(recent.len(), 5);
        assert_eq!(recent[0].bc_id, "bc-00000006-1111-4111-8111-111111111111");
        assert_eq!(recent[4].bc_id, "bc-00000002-1111-4111-8111-111111111111");
    }
}
