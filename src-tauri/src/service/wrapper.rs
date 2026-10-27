use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::cli::config::{configured_binary, load as load_cli};
use crate::cli::paths::{default_search_paths, is_usable_file, look_on_path};
use crate::service::command::build_launch_plan;
use crate::worker::config::load_records;
use crate::worker::error::WorkerError;
use crate::worker::paths::{WRAPPER_LOG, worker_data_dir};

const WRAPPER_LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

pub fn wrapper_install_path() -> Result<PathBuf, WorkerError> {
    if cfg!(windows) {
        let local = std::env::var("LOCALAPPDATA")
            .map_err(|_| WorkerError::msg("%LOCALAPPDATA% is not set"))?;
        return Ok(PathBuf::from(local)
            .join("StandEasyOps")
            .join("WorkerDock")
            .join("bin")
            .join("worker-dock-run.exe"));
    }
    let home =
        crate::cli::paths::home_dir().map_err(|error| WorkerError::msg(error.to_string()))?;
    Ok(home.join(".local").join("bin").join("worker-dock-run"))
}

pub fn install_wrapper() -> Result<PathBuf, WorkerError> {
    let dest = wrapper_install_path()?;
    if let Some(parent) = dest.parent() {
        crate::privacy::ensure_private_dir(parent)?;
    }
    let src = std::env::current_exe()?;
    let same = src
        .canonicalize()
        .ok()
        .zip(dest.canonicalize().ok())
        .is_some_and(|(a, b)| a == b);
    if !same {
        match fs::copy(&src, &dest) {
            Ok(_) => {}
            Err(error) if dest.is_file() => {
                let _ = error;
            }
            Err(error) => return Err(error.into()),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
        }
    }
    Ok(dest)
}

pub fn resolve_agent_sync() -> Result<PathBuf, WorkerError> {
    let config = load_cli().map_err(|error| WorkerError::msg(error.to_string()))?;
    let mut candidates = Vec::new();
    if let Some(configured) = configured_binary(&config) {
        candidates.push(configured.to_path_buf());
    }
    if let Ok(defaults) = default_search_paths() {
        candidates.extend(defaults);
    }
    if let Some(from_path) = look_on_path("agent") {
        candidates.push(from_path);
    }
    for candidate in candidates {
        if is_usable_file(&candidate) {
            return Ok(crate::process::prefer_hidden_agent(&candidate));
        }
    }
    Err(WorkerError::msg("Install the official agent CLI first."))
}

/// Run the official CLI as this process. Used when the installed
/// `worker-dock-run` binary (a copy of worker-dock) is started by systemd or
/// Task Scheduler. Do not Debug-format the command after the API key env is set.
pub fn run_supervised_worker(id: &str) -> Result<(), WorkerError> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(WorkerError::msg("supervised run requires a worker UUID"));
    }
    let record = load_records()?
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| WorkerError::msg("No worker with that id is saved in config.toml."))?;
    let agent = resolve_agent_sync()?;
    let plan = build_launch_plan(&record, &agent)?;
    super::applied::write_from_args(id, &plan.args)?;
    let mut child = spawn_logged_agent(&plan)?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(WorkerError::msg(format!(
            "official CLI exited {}",
            status.code().unwrap_or(-1)
        )))
    }
}

/// Spawn the official CLI with stdout/stderr copied through a redacting writer.
/// The wrapper stays the parent so secrets are stripped before they hit wrapper.log.
pub fn spawn_logged_agent(plan: &crate::service::command::LaunchPlan) -> Result<Child, WorkerError> {
    let log = open_wrapper_log(plan)?;
    let mut command = prepare_agent_command(plan)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(WorkerError::from)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| WorkerError::msg("official CLI stdout was not piped"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| WorkerError::msg("official CLI stderr was not piped"))?;
    let log_err = log.try_clone()?;
    std::thread::spawn(move || copy_redacted(stdout, log));
    std::thread::spawn(move || copy_redacted(stderr, log_err));
    Ok(child)
}

/// Shared spawn setup for service exec and in-app child mode. Never puts
/// `--api-key` on argv. `CURSOR_API_KEY` is set only when auth method is api_key.
pub fn prepare_agent_command(plan: &crate::service::command::LaunchPlan) -> Result<Command, WorkerError> {
    let agent = crate::process::prefer_hidden_agent(&plan.agent);
    let mut command = Command::new(&agent);
    command
        .args(&plan.args)
        .current_dir(&plan.work_dir)
        .env_remove("CURSOR_API_KEY")
        .stdin(Stdio::null());
    crate::process::hide_std_window(&mut command);
    match crate::auth::api_key_for_spawn() {
        Ok(Some(key)) => {
            command.env("CURSOR_API_KEY", key);
        }
        Ok(None) => {}
        Err(error) => {
            return Err(WorkerError::msg(error.to_string()));
        }
    }
    Ok(command)
}

pub fn open_wrapper_log(plan: &crate::service::command::LaunchPlan) -> Result<std::fs::File, WorkerError> {
    crate::privacy::ensure_private_dir(&plan.data_dir)?;
    let log_path = plan.data_dir.join(WRAPPER_LOG);
    rotate_if_needed(&log_path)?;
    let mut log = crate::privacy::open_private_append(&log_path)?;
    writeln!(
        log,
        "starting {} {}",
        plan.agent.display(),
        plan.args
            .iter()
            .map(|arg| format_arg_for_log(arg))
            .collect::<Vec<_>>()
            .join(" ")
    )?;
    Ok(log)
}

fn copy_redacted(mut reader: impl Read, mut writer: impl Write) {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 8192];
    loop {
        match reader.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        while let Some(idx) = buf.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buf.drain(..=idx).collect();
            let _ = writer.write_all(crate::redact::redact(&String::from_utf8_lossy(&line)).as_bytes());
        }
        if buf.len() > 64 * 1024 {
            let flushed = crate::redact::redact(&String::from_utf8_lossy(&buf));
            let _ = writer.write_all(flushed.as_bytes());
            buf.clear();
        }
    }
    if !buf.is_empty() {
        let _ = writer.write_all(crate::redact::redact(&String::from_utf8_lossy(&buf)).as_bytes());
    }
    let _ = writer.flush();
}

fn format_arg_for_log(arg: &str) -> String {
    if arg.contains(char::is_whitespace) {
        format!("\"{arg}\"")
    } else {
        arg.to_string()
    }
}

fn rotate_if_needed(path: &Path) -> Result<(), WorkerError> {
    let Ok(meta) = fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() < WRAPPER_LOG_ROTATE_BYTES {
        return Ok(());
    }
    let rotated = path.with_extension("log.1");
    let _ = fs::remove_file(&rotated);
    fs::rename(path, rotated)?;
    Ok(())
}

pub fn ensure_data_dir(id: &str) -> Result<PathBuf, WorkerError> {
    let dir = worker_data_dir(id)?;
    crate::privacy::ensure_private_dir(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_redacted_strips_tokens() {
        use std::io::Cursor;
        let input = Cursor::new("Authorization: Bearer eyJabc.def.ghi\nkeep this\n");
        let mut out = Vec::new();
        copy_redacted(input, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("keep this"));
        assert!(!text.contains("eyJabc.def.ghi"));
        assert!(text.contains("Bearer ***"));
    }

    #[test]
    fn wrapper_name_matches_design() {
        let path = wrapper_install_path().unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("worker-dock-run"));
        assert!(!name.to_ascii_lowercase().contains("cursor"));
    }
}
