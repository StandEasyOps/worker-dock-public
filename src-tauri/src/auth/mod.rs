//! Browser login and personal API key storage.
//!
//! Team Pools and service-account keys are out of scope. worker-dock never
//! writes the official `auth.json` and never puts a key on argv.

mod config;
mod error;
mod keychain;
mod login;

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tokio::sync::Mutex as AsyncMutex;

use crate::cli::detect::{detect, run_agent_with_env, with_auto_update_flag};
use config::{METHOD_API_KEY, METHOD_CLI_LOGIN, load_auth_method, save_auth_method};
use error::AuthError;
use keychain::{delete_api_key, has_api_key, load_api_key, save_api_key, validate_personal_key};
use login::{cancel_session, is_active, start_browser_login};

pub use login::AuthState;

/// `status --format json` (and optional `about`) at most every 20s unless
/// the UI asks for a forced refresh (window focus / sign-in / sign-out).
const STATUS_TTL: Duration = Duration::from_secs(20);

static STATUS_CACHE: Mutex<Option<(Instant, AuthStatus)>> = Mutex::new(None);
static STATUS_FLIGHT: OnceLock<AsyncMutex<()>> = OnceLock::new();

pub fn invalidate_auth_cache() {
    if let Ok(mut guard) = STATUS_CACHE.lock() {
        *guard = None;
    }
}

pub(crate) fn store_auth_cache(status: &AuthStatus) {
    if let Ok(mut guard) = STATUS_CACHE.lock() {
        *guard = Some((Instant::now(), status.clone()));
    }
}

fn cached_status(browser_login_active: bool) -> Option<AuthStatus> {
    let Ok(guard) = STATUS_CACHE.lock() else {
        return None;
    };
    let (at, value) = guard.as_ref()?;
    if !is_fresh(*at, Instant::now(), STATUS_TTL) {
        return None;
    }
    let mut cloned = value.clone();
    cloned.browser_login_active = browser_login_active;
    Some(cloned)
}

fn is_fresh(at: Instant, now: Instant, ttl: Duration) -> bool {
    now.saturating_duration_since(at) < ttl
}

/// Keychain API key for redaction, whatever the configured auth method is.
pub(crate) fn stored_api_key() -> Option<String> {
    load_api_key().ok().flatten()
}

/// Injected as `CURSOR_API_KEY` only when `[auth].method = "api_key"`.
pub(crate) fn api_key_for_spawn() -> Result<Option<String>, AuthError> {
    match load_auth_method() {
        Ok(method) if method.as_deref() == Some(METHOD_API_KEY) => load_api_key(),
        Ok(_) => Ok(None),
        Err(error) => Err(AuthError::msg(error.to_string())),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub cli_present: bool,
    pub signed_in: bool,
    pub method: Option<String>,
    pub user_email: Option<String>,
    pub has_api_key: bool,
    pub is_authenticated: bool,
    pub message: Option<String>,
    pub browser_login_active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthProgress {
    pub phase: String,
    pub detail: String,
    pub login_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StatusJson {
    #[serde(default)]
    #[allow(dead_code)]
    status: Option<String>,
    #[serde(rename = "isAuthenticated", default)]
    is_authenticated: bool,
    #[serde(rename = "hasAccessToken", default)]
    #[allow(dead_code)]
    has_access_token: Option<bool>,
    #[serde(rename = "hasRefreshToken", default)]
    #[allow(dead_code)]
    has_refresh_token: Option<bool>,
    #[serde(default)]
    message: Option<String>,
}

#[tauri::command]
pub async fn auth_status(
    state: State<'_, AuthState>,
    force: Option<bool>,
) -> Result<AuthStatus, AuthError> {
    current_status(&state, force.unwrap_or(false)).await
}

#[tauri::command]
pub async fn auth_start_browser_login(
    app: AppHandle,
    state: State<'_, AuthState>,
) -> Result<AuthStatus, AuthError> {
    start_browser_login(&app, &state).await
}

#[tauri::command]
pub async fn auth_cancel_browser_login(
    state: State<'_, AuthState>,
) -> Result<AuthStatus, AuthError> {
    cancel_session(&state).await;
    invalidate_auth_cache();
    current_status(&state, true).await
}

#[tauri::command]
pub async fn auth_save_api_key(
    state: State<'_, AuthState>,
    key: String,
) -> Result<AuthStatus, AuthError> {
    cancel_session(&state).await;
    let validated = validate_personal_key(&key)?.to_string();
    drop(key);
    tokio::task::spawn_blocking(move || save_api_key(&validated))
        .await
        .map_err(|error| AuthError::msg(format!("keychain task failed: {error}")))??;
    save_auth_method(METHOD_API_KEY)?;
    invalidate_auth_cache();
    current_status(&state, true).await
}

#[tauri::command]
pub async fn auth_sign_out(state: State<'_, AuthState>) -> Result<AuthStatus, AuthError> {
    cancel_session(&state).await;
    let config = crate::cli::config::load()?;
    if let Some(detected) = detect(&config).await? {
        let args = with_auto_update_flag(detected.auto_update_flag_supported, &["logout"]);
        let _ = run_agent_with_env(&detected.path, &args, None).await;
    }
    tokio::task::spawn_blocking(delete_api_key)
        .await
        .map_err(|error| AuthError::msg(format!("keychain task failed: {error}")))??;
    crate::cli::detect::invalidate_detect_cache();
    invalidate_auth_cache();
    current_status(&state, true).await
}

async fn current_status(state: &AuthState, force: bool) -> Result<AuthStatus, AuthError> {
    let active = is_active(state).await;
    if !force {
        if let Some(cached) = cached_status(active) {
            return Ok(cached);
        }
    }
    let _flight = STATUS_FLIGHT
        .get_or_init(|| AsyncMutex::new(()))
        .lock()
        .await;
    if !force {
        if let Some(cached) = cached_status(active) {
            return Ok(cached);
        }
    }
    let status = build_status(active).await?;
    store_auth_cache(&status);
    Ok(status)
}

pub(crate) async fn build_status(browser_login_active: bool) -> Result<AuthStatus, AuthError> {
    let config = crate::cli::config::load()?;
    let detected = detect(&config).await?;
    let stored_method = load_auth_method()?;
    let (has_api_key, keychain_error) = match keychain_has().await {
        Ok(value) => (value, None),
        Err(error) => (false, Some(error.to_string())),
    };

    let Some(found) = detected else {
        return Ok(AuthStatus {
            cli_present: false,
            signed_in: has_api_key,
            method: stored_method,
            user_email: None,
            has_api_key,
            is_authenticated: false,
            message: Some(
                keychain_error
                    .unwrap_or_else(|| "Install the official agent CLI first.".to_string()),
            ),
            browser_login_active,
        });
    };

    let inject = if stored_method.as_deref() == Some(METHOD_API_KEY) {
        keychain_load().await.ok().flatten()
    } else {
        None
    };

    let status_args = with_auto_update_flag(
        found.auto_update_flag_supported,
        &["status", "--format", "json"],
    );
    let (is_authenticated, mut message) =
        match run_agent_with_env(&found.path, &status_args, inject.as_deref()).await {
            Ok(output) => match serde_json::from_str::<StatusJson>(&output) {
                Ok(parsed) => (parsed.is_authenticated, parsed.message),
                Err(_) => (false, Some("Official CLI status was not JSON.".to_string())),
            },
            Err(error) => (false, Some(error.to_string())),
        };

    let mut user_email = None;
    if is_authenticated || inject.is_some() {
        if let Ok(about) = crate::cli::detect::about_with_env(
            &found.path,
            found.auto_update_flag_supported,
            inject.as_deref(),
        )
        .await
        {
            user_email = about
                .user_email
                .filter(|email| !email.is_empty() && email != "Not logged in");
        }
    }

    if has_api_key && !is_authenticated {
        message = Some(
            "A personal API key is stored in the OS keychain. The official CLI has not confirmed it yet."
                .to_string(),
        );
    }
    if stored_method.as_deref() == Some(METHOD_API_KEY) {
        if let Some(error) = keychain_error {
            message = Some(error);
        }
    }

    let method = stored_method.or_else(|| {
        if is_authenticated {
            Some(METHOD_CLI_LOGIN.to_string())
        } else {
            None
        }
    });

    Ok(AuthStatus {
        cli_present: true,
        signed_in: is_authenticated || has_api_key,
        method,
        user_email,
        has_api_key,
        is_authenticated,
        message,
        browser_login_active,
    })
}

async fn keychain_has() -> Result<bool, AuthError> {
    tokio::task::spawn_blocking(has_api_key)
        .await
        .map_err(|error| AuthError::msg(format!("keychain task failed: {error}")))?
}

async fn keychain_load() -> Result<Option<String>, AuthError> {
    tokio::task::spawn_blocking(load_api_key)
        .await
        .map_err(|error| AuthError::msg(format!("keychain task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unauthenticated_status_json() {
        let parsed: StatusJson = serde_json::from_str(
            r#"{
              "status": "unauthenticated",
              "isAuthenticated": false,
              "hasAccessToken": false,
              "hasRefreshToken": false,
              "message": "Not logged in"
            }"#,
        )
        .unwrap();
        assert!(!parsed.is_authenticated);
        assert_eq!(parsed.message.as_deref(), Some("Not logged in"));
        assert_eq!(parsed.status.as_deref(), Some("unauthenticated"));
    }

    #[test]
    fn parses_authenticated_status_json() {
        let parsed: StatusJson =
            serde_json::from_str(r#"{ "status": "authenticated", "isAuthenticated": true }"#)
                .unwrap();
        assert!(parsed.is_authenticated);
    }

    #[test]
    fn status_ttl_is_twenty_seconds() {
        assert_eq!(STATUS_TTL, Duration::from_secs(20));
        let at = Instant::now();
        assert!(is_fresh(at, at + Duration::from_secs(19), STATUS_TTL));
        assert!(!is_fresh(at, at + STATUS_TTL, STATUS_TTL));
    }

    #[test]
    fn cache_invalidation_drops_stale_status() {
        store_auth_cache(&AuthStatus {
            cli_present: true,
            signed_in: false,
            method: None,
            user_email: None,
            has_api_key: false,
            is_authenticated: false,
            message: Some("Not logged in".into()),
            browser_login_active: false,
        });
        assert!(cached_status(false).is_some());
        invalidate_auth_cache();
        assert!(cached_status(false).is_none());
    }
}
