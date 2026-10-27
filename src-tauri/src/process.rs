//! Hidden child processes, PID tracking, and Windows console suppression.
//!
//! Official `agent.cmd` opens a visible console via `cmd.exe` → PowerShell →
//! node. Prefer `cursor-agent.exe` / `agent.exe` and set `CREATE_NO_WINDOW` on
//! every Windows spawn so version/status polls never flash a window. Track
//! poll/login/debug PIDs so a hung check can be killed, and so Quit can reap
//! leftovers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

/// `CREATE_NO_WINDOW` — hide the console for `cmd`, PowerShell, and GUI-less
/// Win32 children. Must be applied on the first process we create; a hidden
/// `cmd.exe` still hides the rest of the official CLI chain.
#[cfg_attr(not(windows), allow(dead_code))]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static CHILDREN: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Prefer a real `.exe` next to a `.cmd` / `.bat` so we never start `cmd.exe`.
pub fn prefer_hidden_agent(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "cmd" || ext == "bat" {
            if let Some(stem) = path.file_stem() {
                let exe = path.with_file_name(format!("{}.exe", stem.to_string_lossy()));
                if exe.is_file() {
                    return exe;
                }
            }
            for name in ["cursor-agent.exe", "agent.exe"] {
                let candidate = path.with_file_name(name);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    path.to_path_buf()
}

/// Hide a `std::process::Command` console on Windows. No-op elsewhere.
pub fn hide_std_window(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

/// Hide a `tokio::process::Command` console on Windows. No-op elsewhere.
pub fn hide_tokio_window(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

pub fn register_pid(pid: u32) {
    if let Ok(mut set) = CHILDREN.lock() {
        set.insert(pid);
    }
}

pub fn unregister_pid(pid: u32) {
    if let Ok(mut set) = CHILDREN.lock() {
        set.remove(&pid);
    }
}

/// Record the OS pid of a just-spawned tokio child (poll, login, debug).
pub fn track_tokio_pid(child: &tokio::process::Child) -> Option<u32> {
    let pid = child.id();
    if let Some(pid) = pid {
        register_pid(pid);
    }
    pid
}

pub fn finish_tracked_pid(pid: Option<u32>, timed_out: bool) {
    if let Some(pid) = pid {
        if timed_out {
            kill_process_tree(pid);
        }
        unregister_pid(pid);
    }
}

/// Kill every tracked poll/login/debug child (and its Windows process tree).
pub fn kill_all_children() {
    let pids: Vec<u32> = CHILDREN
        .lock()
        .map(|set| set.iter().copied().collect())
        .unwrap_or_default();
    for pid in pids {
        kill_process_tree(pid);
    }
    if let Ok(mut set) = CHILDREN.lock() {
        set.clear();
    }
}

pub fn kill_process_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("taskkill");
        command.args(["/F", "/T", "/PID", &pid.to_string()]);
        hide_std_window(&mut command);
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let _ = command.status();
    }
    #[cfg(unix)]
    {
        let mut term = std::process::Command::new("kill");
        term.args(["-TERM", "--", &pid.to_string()]);
        term.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let _ = term.status();
        let mut kids = std::process::Command::new("pkill");
        kids.args(["-TERM", "-P", &pid.to_string()]);
        kids.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let _ = kids.status();
    }
}

/// Wait for a tracked tokio child. On timeout, kill the process tree so a
/// stuck `agent.cmd` → node chain cannot outlive the poll.
pub async fn wait_tracked_output(
    child: tokio::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, std::io::Error> {
    let pid = track_tokio_pid(&child);
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(result) => {
            finish_tracked_pid(pid, false);
            result
        }
        Err(_) => {
            finish_tracked_pid(pid, true);
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "child process timed out",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn create_no_window_is_the_win32_flag() {
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
    }

    #[test]
    fn prefer_hidden_agent_keeps_non_script_paths() {
        let path = PathBuf::from("/usr/bin/cursor-agent");
        assert_eq!(prefer_hidden_agent(&path), path);
    }

    #[test]
    fn prefer_hidden_agent_maps_cmd_to_sibling_exe() {
        let dir = tempdir().unwrap();
        let cmd = dir.path().join("agent.cmd");
        let exe = dir.path().join("agent.exe");
        fs::write(&cmd, "@echo off\n").unwrap();
        fs::write(&exe, "").unwrap();
        let resolved = prefer_hidden_agent(&cmd);
        #[cfg(windows)]
        assert_eq!(resolved, exe);
        #[cfg(not(windows))]
        assert_eq!(resolved, cmd);
    }

    #[test]
    fn prefer_hidden_agent_maps_cmd_to_cursor_agent_exe() {
        let dir = tempdir().unwrap();
        let cmd = dir.path().join("agent.cmd");
        let exe = dir.path().join("cursor-agent.exe");
        fs::write(&cmd, "@echo off\n").unwrap();
        fs::write(&exe, "").unwrap();
        let resolved = prefer_hidden_agent(&cmd);
        #[cfg(windows)]
        assert_eq!(resolved, exe);
        #[cfg(not(windows))]
        assert_eq!(resolved, cmd);
    }

    #[test]
    fn pid_registry_is_idempotent() {
        register_pid(4_294_967_294);
        register_pid(4_294_967_294);
        unregister_pid(4_294_967_294);
        unregister_pid(4_294_967_294);
    }

    #[test]
    fn unregister_drops_pid_before_a_later_kill() {
        let pid = 4_294_967_293;
        register_pid(pid);
        assert!(CHILDREN.lock().unwrap().contains(&pid));
        unregister_pid(pid);
        let tracked = CHILDREN.lock().unwrap();
        assert!(!tracked.contains(&pid));
    }
}
