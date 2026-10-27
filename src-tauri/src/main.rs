// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args();
    let exe = args.next().unwrap_or_default();
    let first = args.next();
    if first.as_deref() == Some("--supervised-run") {
        let id = args.next().unwrap_or_default();
        if let Err(error) = worker_dock_lib::run_supervised_worker(&id) {
            eprintln!("{}", worker_dock_lib::redact_for_console(&error.to_string()));
            std::process::exit(1);
        }
        return;
    }
    if is_wrapper_name(&exe) {
        let id = first.unwrap_or_default();
        if let Err(error) = worker_dock_lib::run_supervised_worker(&id) {
            eprintln!("{}", worker_dock_lib::redact_for_console(&error.to_string()));
            std::process::exit(1);
        }
        return;
    }
    worker_dock_lib::run();
}

fn is_wrapper_name(exe: &str) -> bool {
    std::path::Path::new(exe)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == "worker-dock-run")
}
