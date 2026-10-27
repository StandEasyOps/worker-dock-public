//! worker-dock application host.
//!
//! CLI lifecycle, sign-in, one named worker, per-user start/stop, status,
//! live logs, diagnostics, and the Home / Settings UI live here.
//! Direct in-app mode is a child of this process and stops when the app quits.

use tauri::Manager;

mod app_info;
mod auth;
mod brand;
mod cli;
mod diag;
mod links;
mod logs;
mod privacy;
mod process;
mod redact;
mod service;
mod tray;
mod worker;

/// Redact secrets before they are printed on the wrapper console.
pub fn redact_for_console(text: &str) -> String {
    redact::redact(text)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tray::show_window(app);
        }))
        .manage(auth::AuthState::default())
        .manage(service::DirectWorker::default())
        .manage(service::Watchdog::default())
        .setup(|app| {
            let handle = app.handle().clone();
            tighten_existing_layout();
            #[cfg(unix)]
            if let Err(error) = service::refresh_linux_unit_if_installed() {
                eprintln!("could not refresh systemd user unit: {error}");
            }
            tray::hide_on_close(&handle);
            if let Err(error) = tray::install(&handle) {
                eprintln!("tray unavailable: {error}");
            }
            spawn_watchdog(handle.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info::app_version,
            cli::cli_status,
            cli::cli_install,
            cli::cli_update,
            auth::auth_status,
            auth::auth_start_browser_login,
            auth::auth_cancel_browser_login,
            auth::auth_save_api_key,
            auth::auth_sign_out,
            worker::worker_get,
            worker::worker_save,
            worker::worker_pick_dir,
            worker::worker_preview_command,
            worker::worker_open_config_folder,
            worker::worker_install,
            worker::worker_uninstall,
            worker::worker_start,
            worker::worker_stop,
            worker::worker_restart,
            worker::worker_set_autostart,
            worker::worker_enable_linger,
            worker::worker_start_direct,
            worker::worker_stop_direct,
            worker::worker_restart_direct,
            worker::worker_reset_identity,
            worker::worker_copy_diagnostics,
            logs::logs_read,
            logs::logs_open_folder,
            diag::diag_run,
            tray::tray_update,
            links::open_external_url
        ])
        .build(tauri::generate_context!())
        .expect("error while running worker-dock")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<service::Watchdog>().clear();
                app.state::<service::DirectWorker>().stop_on_app_quit();
                process::kill_all_children();
            }
        });
}

/// Entry for the installed `worker-dock-run` copy / `--supervised-run`.
pub fn run_supervised_worker(id: &str) -> Result<(), worker::error::WorkerError> {
    service::run_supervised_worker(id)
}

fn spawn_watchdog(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(service::watchdog::poll_interval()).await;
            let direct = handle.state::<service::DirectWorker>().inner().clone();
            let watchdog = handle.state::<service::Watchdog>().inner().clone();
            service::watchdog::tick(&direct, &watchdog).await;
        }
    });
}

/// Restrict leftover 0.1.0 dirs/files (0775/0664) to 0700/0600. Best-effort.
fn tighten_existing_layout() {
    if let Ok(dir) = cli::paths::config_dir() {
        if let Err(error) = privacy::tighten_tree(&dir) {
            eprintln!("could not tighten {}: {error}", dir.display());
        }
    }
    if let Ok(workers) = worker::paths::workers_root() {
        let data = workers.parent().unwrap_or(workers.as_path());
        if let Err(error) = privacy::tighten_tree(data) {
            eprintln!("could not tighten {}: {error}", data.display());
        }
    }
}
