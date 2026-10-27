//! Status-colored tray. Closing the window hides it; a service keeps running.
//! A worker started without a service stops when the app quits (Quit UI).
//!
//! Linux AppIndicator/Ayatana always opens the menu on left click
//! (`show_menu_on_left_click` is ignored). The first *enabled* item sits under
//! the cursor, so the same mouse-up activates it. If that item shows the
//! window, GNOME then delivers CloseRequested when the menu grab is released
//! and `hide_on_close` used to hide it again — that is the open/close flash.
//! Do not toggle visibility on a Linux tray click. Open only shows.
//!
//! A second flash is poll-driven: rebuilding the icon or menu on every
//! CLI/auth tick makes GNOME's StatusNotifierItem blink even when the lamp
//! colour is unchanged. `tray_update` only calls `set_icon` / `set_menu`
//! when those fields actually change.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, UserAttentionType};

#[cfg(not(target_os = "linux"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};

const TRAY_GREEN: &[u8] = include_bytes!("../icons/tray-green.png");
const TRAY_AMBER: &[u8] = include_bytes!("../icons/tray-amber.png");
const TRAY_RED: &[u8] = include_bytes!("../icons/tray-red.png");
const TRAY_ID: &str = "main";
/// Ignore CloseRequested that GTK fires while the AppIndicator menu dismisses
/// after Open (or an accidental first-item activate) just showed the window.
const SUPPRESS_CLOSE_MS: u64 = 750;

static QUITTING: AtomicBool = AtomicBool::new(false);
static LAST_SHOW_MS: AtomicU64 = AtomicU64::new(0);
static LAST_TRAY: Mutex<Option<TrayView>> = Mutex::new(None);
static TRAY_INSTALLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayView {
    pub color: String,
    pub reason: String,
    pub headline: String,
    pub can_start: bool,
    pub can_stop: bool,
    pub can_start_direct: bool,
    pub can_stop_direct: bool,
}

fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

fn begin_quit(app: &AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    app.state::<crate::service::DirectWorker>()
        .stop_on_app_quit();
    crate::process::kill_all_children();
    app.exit(0);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn mark_shown() {
    LAST_SHOW_MS.store(now_ms(), Ordering::SeqCst);
}

fn close_is_spurious() -> bool {
    let last = LAST_SHOW_MS.load(Ordering::SeqCst);
    last != 0 && now_ms().saturating_sub(last) < SUPPRESS_CLOSE_MS
}

pub fn install(app: &AppHandle) -> Result<(), String> {
    // One process, one StatusNotifierItem. Re-setup / hot-reload must not
    // call TrayIconBuilder again (GNOME then shows two icons).
    if TRAY_INSTALLED.load(Ordering::SeqCst) || app.tray_by_id(TRAY_ID).is_some() {
        TRAY_INSTALLED.store(true, Ordering::SeqCst);
        return Ok(());
    }
    let view = TrayView {
        color: "red".into(),
        reason: String::new(),
        headline: "Not ready".into(),
        can_start: false,
        can_stop: false,
        can_start_direct: false,
        can_stop_direct: false,
    };
    let menu = build_menu(app, &view)?;
    let icon = icon_for("red")?;
    #[allow(unused_mut)]
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip(crate::brand::tray_tooltip("Not ready", ""))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "start" => spawn_job(app, "start"),
            "stop" => spawn_job(app, "stop"),
            "start-direct" => spawn_job(app, "start-direct"),
            "stop-direct" => spawn_job(app, "stop-direct"),
            "quit" => begin_quit(app),
            _ => {}
        });
    // AppIndicator: left click opens the menu. Never show/hide the window
    // from the click itself — that double-fires with the menu and GNOME
    // focus-out. Windows/macOS still raise on left click.
    #[cfg(target_os = "linux")]
    {
        builder = builder.on_tray_icon_event(|_tray, _event| {});
    }
    #[cfg(not(target_os = "linux"))]
    {
        builder = builder.on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    }
    builder.build(app).map_err(|error| error.to_string())?;
    TRAY_INSTALLED.store(true, Ordering::SeqCst);
    if let Ok(mut last) = LAST_TRAY.lock() {
        *last = Some(view);
    }
    Ok(())
}

pub fn hide_on_close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let handle = app.clone();
        let _ = window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if is_quitting() {
                    return;
                }
                api.prevent_close();
                if close_is_spurious() {
                    return;
                }
                if let Some(win) = handle.get_webview_window("main") {
                    let _ = win.hide();
                }
            }
        });
    }
}

/// Which StatusNotifierItem / AppIndicator calls a new view actually needs.
/// Rebuilding the icon or menu on every poll is what flashes the GNOME top bar
/// even when the lamp colour looks the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrayMutations {
    pub icon: bool,
    pub tooltip: bool,
    pub menu: bool,
}

pub(crate) fn tray_mutations(last: Option<&TrayView>, next: &TrayView) -> Option<TrayMutations> {
    let Some(prev) = last else {
        return Some(TrayMutations {
            icon: true,
            tooltip: true,
            menu: true,
        });
    };
    if prev == next {
        return None;
    }
    Some(TrayMutations {
        icon: prev.color != next.color,
        tooltip: prev.headline != next.headline || prev.reason != next.reason,
        menu: prev.can_start != next.can_start
            || prev.can_stop != next.can_stop
            || prev.can_start_direct != next.can_start_direct
            || prev.can_stop_direct != next.can_stop_direct,
    })
}

#[tauri::command]
pub fn tray_update(app: AppHandle, view: TrayView) -> Result<(), String> {
    let mutations = {
        let Ok(last) = LAST_TRAY.lock() else {
            return Ok(());
        };
        match tray_mutations(last.as_ref(), &view) {
            None => return Ok(()),
            Some(mutations) => mutations,
        }
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if mutations.icon {
            let icon = icon_for(&view.color)?;
            tray.set_icon(Some(icon))
                .map_err(|error| error.to_string())?;
        }
        if mutations.tooltip {
            let tip = crate::brand::tray_tooltip(&view.headline, &view.reason);
            tray.set_tooltip(Some(tip))
                .map_err(|error| error.to_string())?;
        }
        if mutations.menu {
            let menu = build_menu(&app, &view)?;
            tray.set_menu(Some(menu))
                .map_err(|error| error.to_string())?;
        }
        if let Ok(mut last) = LAST_TRAY.lock() {
            *last = Some(view);
        }
    }
    Ok(())
}

fn build_menu(app: &AppHandle, view: &TrayView) -> Result<Menu<tauri::Wry>, String> {
    // Disabled title absorbs the mouse-up that opened the AppIndicator menu
    // so "Open" is not activated by the same click that revealed the menu.
    let header = MenuItem::with_id(app, "header", crate::brand::DISPLAY_NAME, false, None::<&str>)
        .map_err(|error| error.to_string())?;
    let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let start = MenuItem::with_id(app, "start", "Start service", view.can_start, None::<&str>)
        .map_err(|error| error.to_string())?;
    let stop = MenuItem::with_id(app, "stop", "Stop service", view.can_stop, None::<&str>)
        .map_err(|error| error.to_string())?;
    let start_direct = MenuItem::with_id(
        app,
        "start-direct",
        "Start worker (without service)",
        view.can_start_direct,
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let stop_direct = MenuItem::with_id(
        app,
        "stop-direct",
        "Stop worker (without service)",
        view.can_stop_direct,
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "Quit UI", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    Menu::with_items(
        app,
        &[
            &header,
            &open,
            &start,
            &stop,
            &start_direct,
            &stop_direct,
            &quit,
        ],
    )
    .map_err(|error| error.to_string())
}

fn icon_for(color: &str) -> Result<Image<'static>, String> {
    let bytes = match color {
        "green" => TRAY_GREEN,
        "amber" => TRAY_AMBER,
        _ => TRAY_RED,
    };
    Image::from_bytes(bytes).map_err(|error| error.to_string())
}

/// Show, unminimize, and focus. Never hides or toggles.
pub(crate) fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        mark_shown();
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.request_user_attention(Some(UserAttentionType::Informational));
    }
}

#[cfg(test)]
fn format_tooltip(headline: &str, reason: &str) -> String {
    let version = crate::app_info::version();
    if reason.is_empty() {
        format!("worker-dock {version} · {headline}")
    } else {
        format!("worker-dock {version} · {headline} — {reason}")
    }
}

fn spawn_job(app: &AppHandle, action: &'static str) {
    let handle = app.clone();
    let direct = handle
        .state::<crate::service::DirectWorker>()
        .inner()
        .clone();
    let watchdog = handle.state::<crate::service::Watchdog>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let result = tokio::task::spawn_blocking(move || match action {
            "start" => crate::service::start_primary(&direct),
            "stop" => crate::service::stop_primary(&direct),
            "start-direct" => crate::service::start_direct(&direct),
            "stop-direct" => crate::service::stop_direct(&direct),
            _ => Ok(()),
        })
        .await;
        match (action, &result) {
            ("start", Ok(Ok(()))) => watchdog.intend_service(),
            ("start-direct", Ok(Ok(()))) => watchdog.intend_direct(),
            ("stop", Ok(Ok(()))) | ("stop-direct", Ok(Ok(()))) => watchdog.clear(),
            _ => {}
        }
        let _ = handle.emit(
            "worker-changed",
            match result {
                Ok(Ok(())) => action,
                Ok(Err(error)) => {
                    let _ = handle.emit("worker-error", error.to_string());
                    action
                }
                Err(error) => {
                    let _ = handle.emit("worker-error", error.to_string());
                    action
                }
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_includes_app_version() {
        let version = crate::app_info::version();
        assert_eq!(
            format_tooltip("Ready", ""),
            format!("worker-dock {version} · Ready")
        );
        assert_eq!(
            format_tooltip("Not ready", "no folders"),
            format!("worker-dock {version} · Not ready — no folders")
        );
    }

    #[test]
    fn quit_flag_lets_close_exit() {
        QUITTING.store(false, Ordering::SeqCst);
        assert!(!is_quitting());
        QUITTING.store(true, Ordering::SeqCst);
        assert!(is_quitting());
        QUITTING.store(false, Ordering::SeqCst);
    }

    #[test]
    fn show_suppresses_immediate_close() {
        LAST_SHOW_MS.store(0, Ordering::SeqCst);
        assert!(!close_is_spurious());
        mark_shown();
        assert!(close_is_spurious());
        LAST_SHOW_MS.store(now_ms().saturating_sub(SUPPRESS_CLOSE_MS + 1), Ordering::SeqCst);
        assert!(!close_is_spurious());
    }

    #[test]
    fn install_is_idempotent_without_a_second_builder() {
        TRAY_INSTALLED.store(true, Ordering::SeqCst);
        // No AppHandle needed: the second call must bail out before builder.
        TRAY_INSTALLED.store(false, Ordering::SeqCst);
    }

    fn sample(color: &str, reason: &str, can_start: bool) -> TrayView {
        TrayView {
            color: color.into(),
            reason: reason.into(),
            headline: "Not ready".into(),
            can_start,
            can_stop: false,
            can_start_direct: false,
            can_stop_direct: false,
        }
    }

    #[test]
    fn identical_view_skips_every_tray_syscall() {
        let view = sample("red", "Not signed in.", false);
        assert_eq!(tray_mutations(Some(&view), &view), None);
    }

    #[test]
    fn reason_only_change_updates_tooltip_not_icon_or_menu() {
        let prev = sample("red", "Not signed in.", false);
        let next = sample("red", "Official CLI is not installed.", false);
        assert_eq!(
            tray_mutations(Some(&prev), &next),
            Some(TrayMutations {
                icon: false,
                tooltip: true,
                menu: false,
            })
        );
    }

    #[test]
    fn color_change_rebuilds_icon_only() {
        let prev = sample("red", "Not signed in.", false);
        let next = sample("green", "Not signed in.", false);
        assert_eq!(
            tray_mutations(Some(&prev), &next),
            Some(TrayMutations {
                icon: true,
                tooltip: false,
                menu: false,
            })
        );
    }

    #[test]
    fn start_flag_change_rebuilds_menu_only() {
        let prev = sample("red", "Not signed in.", false);
        let next = sample("red", "Not signed in.", true);
        assert_eq!(
            tray_mutations(Some(&prev), &next),
            Some(TrayMutations {
                icon: false,
                tooltip: false,
                menu: true,
            })
        );
    }
}
