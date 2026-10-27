//! Redact tokens, keys, and auth material before they reach logs or the UI.

/// Replace secrets in CLI output, errors, and log tails shown to the webview.
pub fn redact(text: &str) -> String {
    let mut out = exact_known_values(text);
    out = redact_json_secrets(&out);
    out = redact_assignment_secrets(&out);
    out = redact_headers_and_tokens(&out);
    out
}

/// Also drop an exact known secret (the keychain API key) if the caller has it.
pub fn redact_with_secret(text: &str, secret: Option<&str>) -> String {
    let generic = redact(text);
    match secret {
        Some(secret) if !secret.is_empty() => generic.replace(secret, "***"),
        _ => generic,
    }
}

fn exact_known_values(text: &str) -> String {
    let mut out = text.to_string();
    for key in [
        "CURSOR_API_KEY",
        "CURSOR_AUTH_TOKEN",
        "CURSOR_IDENTITY_TOKEN",
        "CURSOR_IDENTITY_NONCE",
    ] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                out = out.replace(&value, "***");
            }
        }
    }
    out
}

fn redact_json_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = find_json_secret_key(rest) {
        let after_key = &rest[idx..];
        let Some(colon) = after_key.find(':') else {
            out.push_str(&rest[..idx + 1]);
            rest = &rest[idx + 1..];
            continue;
        };
        let value_part = after_key[colon + 1..].trim_start();
        if let Some(stripped) = value_part.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                out.push_str(&rest[..idx]);
                let key_and_colon = &after_key[..=colon];
                out.push_str(key_and_colon);
                out.push_str(" \"***\"");
                rest = &stripped[end + 1..];
                continue;
            }
        }
        out.push_str(&rest[..idx + 1]);
        rest = &rest[idx + 1..];
    }
    out.push_str(rest);
    out
}

fn find_json_secret_key(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    [
        "\"accesstoken\"",
        "\"refresh_token\"",
        "\"refreshtoken\"",
        "\"access_token\"",
        "\"apitoken\"",
        "\"api_key\"",
        "\"apikey\"",
        "\"authtoken\"",
        "\"auth_token\"",
        "\"sessiontoken\"",
        "\"session_token\"",
        "\"id_token\"",
        "\"idtoken\"",
        "\"password\"",
        "\"secret\"",
        "\"authorization\"",
    ]
    .into_iter()
    .filter_map(|key| lower.find(key))
    .min()
}

fn redact_assignment_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        if assignment_key(&lower).is_some() {
            if let Some((head, _)) = split_assignment(line) {
                out.push_str(head);
                out.push_str("***");
                if line.ends_with('\n') {
                    out.push('\n');
                }
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

fn assignment_key(lower_line: &str) -> Option<&'static str> {
    const KEYS: &[&str] = &[
        "cursor_api_key",
        "cursor_auth_token",
        "cursor_identity_token",
        "cursor_identity_nonce",
        "api_key",
        "apikey",
        "auth_token",
        "authtoken",
        "access_token",
        "refresh_token",
        "session_token",
    ];
    KEYS.iter().copied().find(|key| {
        lower_line.starts_with(key)
            && lower_line
                .as_bytes()
                .get(key.len())
                .is_some_and(|byte| matches!(byte, b'=' | b':' | b' '))
    })
}

fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let stripped = line.trim_end_matches('\n');
    let pos = stripped.find(['=', ':'])?;
    Some((&stripped[..=pos], &stripped[pos + 1..]))
}

fn redact_headers_and_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((idx, kind)) = next_token_start(rest) {
        out.push_str(&rest[..idx]);
        match kind {
            TokenKind::Bearer => {
                out.push_str("Bearer ***");
                rest = skip_token(&rest[idx + "bearer ".len()..]);
            }
            TokenKind::Sk => {
                out.push_str("sk-***");
                rest = skip_token(&rest[idx + 3..]);
            }
            TokenKind::Jwt => {
                out.push_str("eyJ***");
                rest = skip_token(&rest[idx + 3..]);
            }
        }
    }
    out.push_str(rest);
    out
}

enum TokenKind {
    Bearer,
    Sk,
    Jwt,
}

fn next_token_start(text: &str) -> Option<(usize, TokenKind)> {
    let lower = text.to_ascii_lowercase();
    let bearer = lower.find("bearer ").map(|idx| (idx, TokenKind::Bearer));
    let sk = text.find("sk-").and_then(|idx| {
        let after = text.as_bytes().get(idx + 3)?;
        if after.is_ascii_alphanumeric() {
            Some((idx, TokenKind::Sk))
        } else {
            None
        }
    });
    let jwt = text.find("eyJ").map(|idx| (idx, TokenKind::Jwt));
    [bearer, sk, jwt].into_iter().flatten().min_by_key(|(idx, _)| *idx)
}

fn skip_token(text: &str) -> &str {
    let end = text
        .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | ')' | ']'))
        .unwrap_or(text.len());
    &text[end..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_headers_keys_and_json() {
        let raw = concat!(
            "Authorization: Bearer eyJabc.def.ghi\n",
            "CURSOR_API_KEY=super-test-secret\n",
            "api_key: sk-abcdefghijklmnopqrstuvwxyz\n",
            r#"{"accessToken":"real-access","user":"a@b.c"}"#,
            "\nnot a secret line\n"
        );
        let cleaned = redact(raw);
        assert!(!cleaned.contains("super-test-secret"));
        assert!(!cleaned.contains("real-access"));
        assert!(!cleaned.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(!cleaned.contains("eyJabc.def.ghi"));
        assert!(cleaned.contains("Bearer ***"));
        assert!(cleaned.contains("CURSOR_API_KEY=***"));
        assert!(cleaned.contains("\"accessToken\": \"***\"") || cleaned.contains("\"accessToken\": \"***\""));
        assert!(cleaned.contains("not a secret line"));
        assert!(cleaned.contains("a@b.c"));
    }

    #[test]
    fn redacts_exact_secret_after_generic() {
        assert_eq!(
            redact_with_secret("token=abcd-known-secret", Some("abcd-known-secret")),
            "token=***"
        );
    }
}
