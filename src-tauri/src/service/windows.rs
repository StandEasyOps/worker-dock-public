#![cfg_attr(not(windows), allow(dead_code))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::service::wrapper::wrapper_install_path;
use crate::worker::error::WorkerError;

pub fn task_name(id: &str) -> Result<String, WorkerError> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(WorkerError::msg(
            "scheduled task name must use a worker UUID",
        ));
    }
    Ok(format!(r"StandEasyOps\WorkerDock\{id}"))
}

pub fn render_task_xml(
    id: &str,
    wrapper: &Path,
    work_dir: &Path,
    autostart: bool,
) -> Result<String, WorkerError> {
    let name = task_name(id)?;
    let command = xml_escape(&wrapper.display().to_string());
    let work = xml_escape(&work_dir.display().to_string());
    let trigger = if autostart { "true" } else { "false" };
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>Stand Easy Ops</Author>
    <Description>worker-dock personal worker</Description>
    <URI>\{name}</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>{trigger}</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>--supervised-run {id}</Arguments>
      <WorkingDirectory>{work}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    ))
}

pub fn install_task(id: &str, work_dir: &Path, autostart: bool) -> Result<(), WorkerError> {
    let wrapper = wrapper_install_path()?;
    let xml = render_task_xml(id, &wrapper, work_dir, autostart)?;
    let dir = std::env::temp_dir().join("worker-dock-tasks");
    crate::privacy::ensure_private_dir(&dir)?;
    let file = dir.join(format!("worker-dock-{id}.xml"));
    crate::privacy::write_private_file(&file, xml)?;
    let name = task_name(id)?;
    let result = schtasks(&[
        "/Create",
        "/TN",
        &name,
        "/XML",
        &file.display().to_string(),
        "/F",
    ]);
    let _ = fs::remove_file(&file);
    result
}

pub fn start(id: &str, work_dir: &Path, autostart: bool) -> Result<(), WorkerError> {
    install_task(id, work_dir, autostart)?;
    let name = task_name(id)?;
    schtasks(&["/Run", "/TN", &name])
}

pub fn stop(id: &str) -> Result<(), WorkerError> {
    let name = task_name(id)?;
    match schtasks(&["/End", "/TN", &name]) {
        Ok(()) => Ok(()),
        Err(error) if missing_task(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn set_enabled(id: &str, work_dir: &Path, enabled: bool) -> Result<(), WorkerError> {
    install_task(id, work_dir, enabled)
}

pub fn is_installed(id: &str) -> bool {
    !matches!(query(id), Ok(TaskState::Missing) | Err(_))
}

pub fn uninstall(id: &str) -> Result<(), WorkerError> {
    let _ = stop(id);
    let name = task_name(id)?;
    match schtasks(&["/Delete", "/TN", &name, "/F"]) {
        Ok(()) => Ok(()),
        Err(error) if missing_task(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn query(id: &str) -> Result<TaskState, WorkerError> {
    let name = task_name(id)?;
    match schtasks_output(&["/Query", "/TN", &name, "/FO", "LIST", "/V"]) {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
            if text.contains("status:") && text.contains("running") {
                Ok(TaskState::Running)
            } else if text.contains("status:") && text.contains("ready") {
                Ok(TaskState::Ready)
            } else if text.contains("could not start") || text.contains("failed") {
                Ok(TaskState::Failed)
            } else {
                Ok(TaskState::Ready)
            }
        }
        Ok(_) | Err(_) => Ok(TaskState::Missing),
    }
}

pub fn logon_trigger_enabled(id: &str) -> bool {
    let Ok(name) = task_name(id) else {
        return false;
    };
    let Ok(output) = schtasks_output(&["/Query", "/TN", &name, "/FO", "LIST", "/V"]) else {
        return false;
    };
    let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    text.contains("logon") && !text.contains("disabled")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Running,
    Ready,
    Failed,
    Missing,
}

pub fn git_bash_present() -> bool {
    let mut candidates = Vec::new();
    for key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Ok(root) = std::env::var(key) {
            if key == "LOCALAPPDATA" {
                candidates.push(
                    PathBuf::from(root)
                        .join("Programs")
                        .join("Git")
                        .join("bin")
                        .join("bash.exe"),
                );
            } else {
                candidates.push(PathBuf::from(root).join("Git").join("bin").join("bash.exe"));
            }
        }
    }
    candidates.iter().any(|path| path.is_file())
}

pub fn git_bash_required_message() -> &'static str {
    "A Windows worker requires Git for Windows, including Git Bash. Install it from https://git-scm.com/download/win."
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn schtasks(args: &[&str]) -> Result<(), WorkerError> {
    let output = schtasks_output(args)?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(WorkerError::msg(if stderr.is_empty() {
            format!("schtasks {} failed", args.join(" "))
        } else {
            stderr
        }))
    }
}

fn schtasks_output(args: &[&str]) -> Result<std::process::Output, WorkerError> {
    let mut command = Command::new("schtasks");
    command.args(args);
    crate::process::hide_std_window(&mut command);
    command.output().map_err(|error| {
        WorkerError::msg(format!(
            "schtasks is not available ({error}). Start/stop needs a real Windows user session."
        ))
    })
}

fn missing_task(error: &WorkerError) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("cannot find") || text.contains("not found") || text.contains("does not exist")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn task_name_matches_design() {
        assert_eq!(
            task_name("8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60").unwrap(),
            r"StandEasyOps\WorkerDock\8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60"
        );
        assert!(task_name("..\\evil").is_err());
    }

    #[test]
    fn xml_is_interactive_user_and_has_no_secret() {
        let xml = render_task_xml(
            "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60",
            Path::new(
                r"C:\Users\dev\AppData\Local\StandEasyOps\WorkerDock\bin\worker-dock-run.exe",
            ),
            Path::new(r"C:\work\app"),
            true,
        )
        .unwrap();
        assert!(xml.contains("InteractiveToken"));
        assert!(xml.contains("LeastPrivilege"));
        assert!(xml.contains("LogonTrigger"));
        assert!(xml.contains("--supervised-run 8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60"));
        assert!(xml.contains("<Count>3</Count>"));
        assert!(!xml.contains("CURSOR_API_KEY"));
        assert!(!xml.contains("--api-key"));
        assert!(!xml.contains("SYSTEM"));
        assert!(!xml.contains("WinSW"));
        let escaped = render_task_xml(
            "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60",
            Path::new(r"C:\path & more\worker-dock-run.exe"),
            Path::new(r"C:\work"),
            false,
        )
        .unwrap();
        assert!(escaped.contains("&amp;"));
        assert!(escaped.contains("<Enabled>false</Enabled>"));
    }
}
