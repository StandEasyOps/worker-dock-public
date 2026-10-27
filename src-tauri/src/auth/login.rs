use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use url::Url;

use crate::auth::config::{METHOD_CLI_LOGIN, save_auth_method};
use crate::auth::error::AuthError;
use crate::auth::{
    AuthProgress, AuthStatus, build_status, invalidate_auth_cache, store_auth_cache,
};
use crate::cli::detect::{DISABLE_AUTO_UPDATE, detect};
use crate::cli::error::CliError;
use crate::process::{hide_tokio_window, prefer_hidden_agent, track_tokio_pid};

const URL_WAIT: Duration = Duration::from_secs(60);
const LOGIN_WAIT: Duration = Duration::from_secs(15 * 60);
const POLL_EVERY: Duration = Duration::from_secs(2);

pub struct BrowserLogin {
    child: Child,
    pid: Option<u32>,
    cancel: Arc<AtomicBool>,
}

impl Drop for BrowserLogin {
    fn drop(&mut self) {
        release_tracked_pid(&mut self.pid);
    }
}

fn release_tracked_pid(pid: &mut Option<u32>) {
    if let Some(pid) = pid.take() {
        crate::process::unregister_pid(pid);
    }
}

#[derive(Default)]
pub struct AuthState {
    session: tokio::sync::Mutex<Option<BrowserLogin>>,
}

pub async fn is_active(state: &AuthState) -> bool {
    let mut guard = state.session.lock().await;
    let Some(session) = guard.as_mut() else {
        return false;
    };
    match session.child.try_wait() {
        Ok(None) => true,
        Ok(Some(_)) | Err(_) => {
            release_tracked_pid(&mut session.pid);
            guard.take();
            false
        }
    }
}

pub async fn cancel_session(state: &AuthState) {
    let mut guard = state.session.lock().await;
    if let Some(mut session) = guard.take() {
        session.cancel.store(true, Ordering::Relaxed);
        let still_running = matches!(session.child.try_wait(), Ok(None));
        if let Some(pid) = session.pid.take() {
            if still_running {
                crate::process::kill_process_tree(pid);
            }
            crate::process::unregister_pid(pid);
        }
        let _ = session.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(3), session.child.wait()).await;
    }
}

pub async fn start_browser_login(
    app: &AppHandle,
    state: &AuthState,
) -> Result<AuthStatus, AuthError> {
    cancel_session(state).await;
    invalidate_auth_cache();

    let config = crate::cli::config::load()?;
    let detected = detect(&config)
        .await?
        .ok_or_else(|| AuthError::msg("Install the official agent CLI first."))?;

    emit(app, "starting", "Starting official CLI login", None);

    let mut args = Vec::new();
    if detected.auto_update_flag_supported {
        args.push(DISABLE_AUTO_UPDATE);
    }
    args.push("login");

    let agent = prefer_hidden_agent(&detected.path);
    let mut command = Command::new(&agent);
    command
        .args(&args)
        .env("NO_OPEN_BROWSER", "1")
        .env_remove("CURSOR_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    hide_tokio_window(&mut command);
    // Do not Debug-format `command` — std Debug can include environment values.

    let mut child = command.spawn().map_err(CliError::from)?;
    let mut pid = track_tokio_pid(&child);
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            release_tracked_pid(&mut pid);
            return Err(AuthError::msg("official CLI login stdout was not piped"));
        }
    };
    if let Some(mut stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut sink = Vec::new();
            let _ = stderr.read_to_end(&mut sink).await;
        });
    }

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.session.lock().await;
        *guard = Some(BrowserLogin {
            child,
            pid,
            cancel: cancel.clone(),
        });
    }

    emit(
        app,
        "waiting_for_url",
        "Waiting for the official sign-in URL",
        None,
    );

    let url = match read_login_url(stdout, &cancel).await {
        Ok(url) => url,
        Err(error) => {
            cancel_session(state).await;
            return Err(error);
        }
    };

    if cancel.load(Ordering::Relaxed) {
        cancel_session(state).await;
        return Err(AuthError::cancelled());
    }

    emit(
        app,
        "opening_browser",
        "Opening the sign-in page",
        Some(url.clone()),
    );

    let open_url = url.clone();
    match tokio::task::spawn_blocking(move || open::that_detached(open_url.as_str())).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) | Err(_) => {
            emit(
                app,
                "opening_browser",
                "Could not open a browser on this machine. Copy the URL below.",
                Some(url.clone()),
            );
        }
    }

    emit(app, "waiting_for_browser", "Waiting for browser", Some(url));

    let deadline = Instant::now() + LOGIN_WAIT;
    loop {
        if cancel.load(Ordering::Relaxed) {
            cancel_session(state).await;
            emit(app, "cancelled", "Cancelled", None);
            return Err(AuthError::cancelled());
        }
        if Instant::now() > deadline {
            cancel_session(state).await;
            return Err(AuthError::msg("Timed out waiting for browser sign-in."));
        }

        match build_status(true).await {
            Ok(status) if status.is_authenticated => {
                save_auth_method(METHOD_CLI_LOGIN)?;
                tokio::time::sleep(Duration::from_millis(400)).await;
                cancel_session(state).await;
                emit(app, "done", "Signed in", None);
                let status = build_status(false).await?;
                store_auth_cache(&status);
                return Ok(status);
            }
            _ => {}
        }

        let login_exited = {
            let mut guard = state.session.lock().await;
            match guard.as_mut() {
                Some(session) => match session.child.try_wait() {
                    Ok(None) => None,
                    Ok(Some(status)) => {
                        release_tracked_pid(&mut session.pid);
                        Some(status.success())
                    }
                    Err(_) => {
                        release_tracked_pid(&mut session.pid);
                        Some(false)
                    }
                },
                None => Some(false),
            }
        };
        if let Some(success) = login_exited {
            if !success {
                cancel_session(state).await;
                return Err(AuthError::msg(
                    "Official CLI login exited before sign-in finished.",
                ));
            }
        }

        tokio::time::sleep(POLL_EVERY).await;
    }
}

async fn read_login_url(
    stdout: tokio::process::ChildStdout,
    cancel: &AtomicBool,
) -> Result<String, AuthError> {
    let mut reader = BufReader::new(stdout);
    let mut collected = String::new();
    let deadline = Instant::now() + URL_WAIT;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AuthError::cancelled());
        }
        if Instant::now() > deadline {
            return Err(AuthError::msg(
                "The official CLI did not print an allowlisted sign-in URL.",
            ));
        }
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_millis(500), reader.read_line(&mut line)).await {
            Ok(Ok(0)) => {
                if let Some(url) = extract_login_url(&collected) {
                    return Ok(url);
                }
                return Err(AuthError::msg(
                    "The official CLI closed login before printing a browser URL.",
                ));
            }
            Ok(Ok(_)) => {
                collected.push_str(&line);
                if collected.len() > 16 * 1024 {
                    collected = collected[collected.len() - 8 * 1024..].to_string();
                }
                if let Some(url) = extract_login_url(&collected) {
                    return Ok(url);
                }
            }
            Ok(Err(error)) => return Err(AuthError::from(CliError::from(error))),
            Err(_) => {}
        }
    }
}

fn emit(app: &AppHandle, phase: &str, detail: impl Into<String>, login_url: Option<String>) {
    let _ = app.emit(
        "auth-progress",
        AuthProgress {
            phase: phase.to_string(),
            detail: detail.into(),
            login_url,
        },
    );
}

/// Only `https://cursor.com/loginDeepControl?…&redirectTarget=cli`.
pub fn is_allowlisted_login_url(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    if url.host_str() != Some("cursor.com") {
        return false;
    }
    if url.port().is_some() && url.port() != Some(443) {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if url.path() != "/loginDeepControl" {
        return false;
    }
    url.query_pairs()
        .any(|(key, value)| key == "redirectTarget" && value == "cli")
}

pub fn extract_login_url(text: &str) -> Option<String> {
    let mut rest = text;
    while let Some(start) = rest.find("https://") {
        let candidate = &rest[start..];
        let end = candidate
            .find(|c: char| c.is_whitespace() || matches!(c, '>' | '"' | '\''))
            .unwrap_or(candidate.len());
        let raw = candidate[..end].trim_end_matches(['.', ',', ';', ')', ']', '>']);
        if let Ok(parsed) = Url::parse(raw) {
            if is_allowlisted_login_url(&parsed) {
                return Some(parsed.to_string());
            }
        }
        rest = &candidate["https://".len()..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPIKE: &str = "Open a browser and navigate to this link: https://cursor.com/loginDeepControl?challenge=abc&uuid=def&mode=login&redirectTarget=cli&supportsSelectedTeamLogin=true";

    #[test]
    fn extracts_spike_login_url() {
        let url = extract_login_url(SPIKE).unwrap();
        let parsed = Url::parse(&url).unwrap();
        assert!(is_allowlisted_login_url(&parsed));
        assert_eq!(parsed.path(), "/loginDeepControl");
    }

    #[test]
    fn rejects_non_allowlisted_urls() {
        assert!(
            extract_login_url("http://cursor.com/loginDeepControl?redirectTarget=cli").is_none()
        );
        assert!(
            extract_login_url("https://evil.example/loginDeepControl?redirectTarget=cli").is_none()
        );
        assert!(extract_login_url("https://cursor.com/loginDeepControl?mode=login").is_none());
        assert!(
            extract_login_url("https://user:pass@cursor.com/loginDeepControl?redirectTarget=cli")
                .is_none()
        );
        assert!(
            extract_login_url("https://cursor.com:8080/loginDeepControl?redirectTarget=cli")
                .is_none()
        );
        assert!(extract_login_url("https://cursor.com/other?redirectTarget=cli").is_none());
    }
}
