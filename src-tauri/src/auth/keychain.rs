use keyring::v1::{Entry, Error as KeyringError};

use crate::auth::error::AuthError;

/// Credential Manager / libsecret service id. Must stay stable.
pub const KEYCHAIN_SERVICE: &str = "ops.standeasy.worker-dock";
/// Account name for the single personal API key we store.
pub const KEYCHAIN_ACCOUNT: &str = "personal-api-key";

fn entry() -> Result<Entry, AuthError> {
    Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).map_err(map_keyring_error)
}

pub fn validate_personal_key(key: &str) -> Result<&str, AuthError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AuthError::msg("Paste a personal API key."));
    }
    if key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AuthError::msg(
            "The API key must be a single token with no spaces or control characters.",
        ));
    }
    // Team / org / service-account keys are rejected conceptually (no pool
    // APIs, no Team Pools). There is no reliable format check; do not invent one.
    Ok(key)
}

pub fn save_api_key(secret: &str) -> Result<(), AuthError> {
    let secret = validate_personal_key(secret)?;
    entry()?.set_password(secret).map_err(map_keyring_error)
}

pub fn load_api_key() -> Result<Option<String>, AuthError> {
    match entry()?.get_password() {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(map_keyring_error(error)),
    }
}

pub fn has_api_key() -> Result<bool, AuthError> {
    Ok(load_api_key()?.is_some())
}

pub fn delete_api_key() -> Result<(), AuthError> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(map_keyring_error(error)),
    }
}

/// Map keyring failures without formatting payloads. `BadEncoding` /
/// `BadDataFormat` can hold secret bytes.
fn map_keyring_error(error: KeyringError) -> AuthError {
    match error {
        KeyringError::NoEntry => {
            AuthError::msg("No personal API key is stored in the OS keychain.")
        }
        KeyringError::NoStorageAccess(_)
        | KeyringError::NoDefaultStore
        | KeyringError::PlatformFailure(_) => AuthError::msg(
            "OS keychain is unavailable. Unlock Secret Service (libsecret / GNOME Keyring) or Windows Credential Manager, or use browser sign-in.",
        ),
        KeyringError::NotSupportedByStore(_)
        | KeyringError::Invalid(_, _)
        | KeyringError::TooLong(_, _)
        | KeyringError::BadStoreFormat(_)
        | KeyringError::Ambiguous(_)
        | KeyringError::BadEncoding(_)
        | KeyringError::BadDataFormat(_, _) => AuthError::msg(
            "OS keychain rejected the request. The personal API key was not written to config or logs.",
        ),
        other => {
            let _ = other;
            AuthError::msg(
                "OS keychain error. The personal API key was not written to config or logs.",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_whitespace_keys() {
        assert!(validate_personal_key("").is_err());
        assert!(validate_personal_key("   ").is_err());
        assert!(validate_personal_key("abc def").is_err());
        assert!(validate_personal_key("abc\ndef").is_err());
        assert_eq!(
            validate_personal_key("  personal-key-token  ").unwrap(),
            "personal-key-token"
        );
    }

    #[test]
    fn keychain_identity_matches_design() {
        assert_eq!(KEYCHAIN_SERVICE, "ops.standeasy.worker-dock");
        assert_eq!(KEYCHAIN_ACCOUNT, "personal-api-key");
    }

    #[test]
    fn keyring_error_messages_do_not_embed_secret_bytes() {
        let encoded = map_keyring_error(KeyringError::BadEncoding(b"super-secret-value".to_vec()));
        let shown = encoded.to_string();
        assert!(!shown.contains("super-secret-value"));
        assert!(shown.contains("OS keychain"));
    }
}
