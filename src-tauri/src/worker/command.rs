use serde::Serialize;

use crate::cli::detect::DISABLE_AUTO_UPDATE;
use crate::worker::config::WorkerRecord;
use crate::worker::error::WorkerError;
use crate::worker::validate::validate_display;

/// Tokens after the official `agent` binary. The service execs
/// `Command::new(agent).args(start_argv)` — never a shell string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartCommand {
    pub agent_path: String,
    pub start_argv: Vec<String>,
    pub command_line: String,
}

/// Build the personal My Machines start command.
///
/// Official `agent worker --help` is `agent worker [options] [command]`. Worker
/// flags belong **before** `start`. `--verbose` is the only `start` flag.
///
/// Published My Machines docs also show `agent worker start --name "my-devbox"`
/// (flags after `start`). Commander accepts that, but we emit the help grammar
/// and the docs’ multi-dir form:
/// `agent worker --name … --worker-dir … --worker-dir … start`.
///
/// Never `--pool`, never `--api-key` on argv.
pub fn personal_start_command(
    agent_path: &str,
    record: &WorkerRecord,
    data_dir: &str,
) -> Result<StartCommand, WorkerError> {
    if record.name.is_empty() {
        return Err(WorkerError::msg(
            "Worker name is required before a start command can be built.",
        ));
    }
    if record.worker_dirs.is_empty() {
        return Err(WorkerError::msg(
            "At least one local --worker-dir is required before a start command can be built.",
        ));
    }
    let binary = if agent_path.trim().is_empty() {
        "agent"
    } else {
        agent_path.trim()
    };
    let mut argv = vec![DISABLE_AUTO_UPDATE.to_string(), "worker".to_string()];
    argv.push("--name".to_string());
    argv.push(record.name.clone());
    for dir in &record.worker_dirs {
        argv.push("--worker-dir".to_string());
        argv.push(dir.clone());
    }
    if !data_dir.is_empty() {
        argv.push("--data-dir".to_string());
        argv.push(data_dir.to_string());
    }
    argv.push("--idle-release-timeout".to_string());
    argv.push(record.idle_release_timeout.to_string());
    argv.push("--management-addr".to_string());
    argv.push(format!("127.0.0.1:{}", record.management_port));
    if record.computer_use {
        argv.push("--computer-use".to_string());
        let display = validate_display(&record.display)?;
        if !display.is_empty() {
            argv.push("--display".to_string());
            argv.push(display);
        }
    }
    if record.connect_workspace && !cfg!(windows) {
        argv.push("--connect-workspace".to_string());
    }
    if record.identity_socket {
        argv.push("--identity-socket".to_string());
    }
    if record.share_desktop {
        argv.push("--share-desktop".to_string());
        argv.push(record.share_desktop_mode.clone());
    }
    argv.push("start".to_string());
    argv.push("--verbose".to_string());
    if argv.iter().any(|arg| {
        arg.contains("--pool")
            || arg.contains("--api-key")
            || arg.contains("mint-github")
            || arg.contains("controller")
    }) {
        return Err(WorkerError::msg(
            "refusing to construct a pool or argv-secret worker command",
        ));
    }
    let command_line = format_command_line(binary, &argv);
    Ok(StartCommand {
        agent_path: binary.to_string(),
        start_argv: argv,
        command_line,
    })
}

pub fn format_command_line(binary: &str, argv: &[String]) -> String {
    let mut parts = Vec::with_capacity(argv.len() + 1);
    parts.push(quote_arg(binary));
    parts.extend(argv.iter().map(|arg| quote_arg(arg)));
    parts.join(" ")
}

/// Display quoting only. The service must run `start_argv` as argv tokens.
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '='))
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::config::WorkerRecord;

    fn record(name: &str, dirs: &[&str]) -> WorkerRecord {
        WorkerRecord::defaults(
            "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60".to_string(),
            name.to_string(),
            dirs.iter().map(|dir| (*dir).to_string()).collect(),
        )
    }

    #[test]
    fn name_and_every_worker_dir_before_start() {
        let command = personal_start_command(
            "/home/dev/.local/bin/agent",
            &record(
                "my-devbox",
                &["/home/dev/repos/app", "/home/dev/repos/infra"],
            ),
            "/home/dev/.local/share/worker-dock/workers/8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60",
        )
        .unwrap();
        assert_eq!(
            command.start_argv,
            vec![
                "--disable-auto-update",
                "worker",
                "--name",
                "my-devbox",
                "--worker-dir",
                "/home/dev/repos/app",
                "--worker-dir",
                "/home/dev/repos/infra",
                "--data-dir",
                "/home/dev/.local/share/worker-dock/workers/8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60",
                "--idle-release-timeout",
                "0",
                "--management-addr",
                "127.0.0.1:18741",
                "start",
                "--verbose",
            ]
        );
        let name_at = command
            .start_argv
            .iter()
            .position(|part| part == "--name")
            .unwrap();
        let start_at = command
            .start_argv
            .iter()
            .position(|part| part == "start")
            .unwrap();
        assert!(name_at < start_at);
        assert_eq!(command.start_argv[1], "worker");
        assert_ne!(command.start_argv.get(2), Some(&"start".to_string()));
        assert!(!command.start_argv.iter().any(|part| part.contains("pool")));
        assert!(
            !command
                .start_argv
                .iter()
                .any(|part| part.contains("api-key"))
        );
        assert!(
            command
                .command_line
                .starts_with("/home/dev/.local/bin/agent ")
        );
        assert!(command.command_line.contains("--name my-devbox"));
        assert!(
            command
                .command_line
                .contains("--worker-dir /home/dev/repos/app")
        );
        assert!(
            command
                .command_line
                .contains("--worker-dir /home/dev/repos/infra")
        );
        assert!(!command.command_line.contains("worker start --name"));
    }

    #[test]
    fn quotes_name_with_spaces_in_display_only() {
        let command =
            personal_start_command("agent", &record("my devbox", &["/tmp/repo"]), "/tmp/data")
                .unwrap();
        assert_eq!(command.start_argv[3], "my devbox");
        assert!(command.command_line.contains("--name 'my devbox'"));
        assert!(!command.start_argv.iter().any(|part| part.contains('\'')));
    }

    #[test]
    fn optional_flags_stay_before_start() {
        let mut rec = record("box", &["/tmp/repo"]);
        rec.computer_use = true;
        rec.display = ":0".to_string();
        rec.identity_socket = true;
        rec.share_desktop = true;
        rec.share_desktop_mode = "view".to_string();
        if !cfg!(windows) {
            rec.connect_workspace = true;
        }
        let command = personal_start_command("agent", &rec, "/tmp/data").unwrap();
        let start_at = command
            .start_argv
            .iter()
            .position(|part| part == "start")
            .unwrap();
        let computer = command
            .start_argv
            .iter()
            .position(|part| part == "--computer-use")
            .unwrap();
        assert!(computer < start_at);
        assert_eq!(command.start_argv[start_at + 1], "--verbose");
    }

    #[test]
    fn refuses_empty_name_or_dirs() {
        assert!(personal_start_command("agent", &record("", &["/tmp/repo"]), "/tmp/data").is_err());
        assert!(personal_start_command("agent", &record("box", &[]), "/tmp/data").is_err());
    }

    #[test]
    fn refuses_display_that_looks_like_a_flag() {
        let mut rec = record("box", &["/tmp/repo"]);
        rec.computer_use = true;
        rec.display = "--pool".to_string();
        assert!(personal_start_command("agent", &rec, "/tmp/data").is_err());
    }
}
