use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::service::wrapper::wrapper_install_path;
use crate::worker::error::WorkerError;

pub fn unit_name(id: &str) -> Result<String, WorkerError> {
    require_uuid(id)?;
    Ok(format!("worker-dock@{id}.service"))
}

pub fn unit_path() -> Result<PathBuf, WorkerError> {
    let home =
        crate::cli::paths::home_dir().map_err(|error| WorkerError::msg(error.to_string()))?;
    let config = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    Ok(config
        .join("systemd")
        .join("user")
        .join("worker-dock@.service"))
}

pub fn render_unit(wrapper: &str) -> Result<String, WorkerError> {
    if !wrapper.starts_with('/') {
        return Err(WorkerError::msg(
            "systemd ExecStart must be an absolute path to worker-dock-run",
        ));
    }
    if wrapper
        .chars()
        .any(|c| c.is_control() || matches!(c, '"' | '\'' | '\\'))
    {
        return Err(WorkerError::msg(
            "wrapper path contains characters systemd cannot take safely",
        ));
    }
    Ok(format!(
        "[Unit]\n\
         Description=worker-dock personal worker %i\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={wrapper} --supervised-run %i\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         {USER_UNIT_HARDENING}\
         # Do not put CURSOR_API_KEY here. The wrapper reads the keychain.\n\
         # Empty capability sets abort a user unit (status 218). Do not make the\n\
         # filesystem read-only (blocks $HOME workspaces) or isolate /tmp (the UI\n\
         # tails the official CLI session logs there).\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    ))
}

/// Directives that work in a stock Ubuntu 24.04 *user* manager without
/// `PrivateUsers=` or other namespace tricks. Empty capability sets are
/// omitted on purpose: an unprivileged user systemd cannot change the
/// bounding set and fails at CAPABILITIES (status 218).
pub const USER_UNIT_HARDENING: &str = "\
NoNewPrivileges=yes\n\
RestrictRealtime=yes\n\
RestrictSUIDSGID=yes\n\
LockPersonality=yes\n\
# AF_NETLINK: the official CLI enumerates network interfaces (getifaddrs).\n\
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK\n\
SystemCallArchitectures=native\n";

/// Rewrite the installed template so an upgraded app drops broken hardening
/// (empty capability sets, ProtectSystem=strict, …) and reloads systemd.
pub fn refresh_installed_unit() -> Result<bool, WorkerError> {
    if !is_installed() {
        return Ok(false);
    }
    install_unit()?;
    Ok(true)
}

pub fn install_unit() -> Result<PathBuf, WorkerError> {
    let wrapper = wrapper_install_path()?;
    let path = unit_path()?;
    if let Some(parent) = path.parent() {
        crate::privacy::ensure_private_dir(parent)?;
    }
    crate::privacy::write_private_file(&path, render_unit(&wrapper.display().to_string())?)?;
    let _ = systemctl(&["daemon-reload"]);
    Ok(path)
}

pub fn start(id: &str) -> Result<(), WorkerError> {
    let unit = unit_name(id)?;
    install_unit()?;
    systemctl(&["start", &unit])?;
    Ok(())
}

pub fn stop(id: &str) -> Result<(), WorkerError> {
    let unit = unit_name(id)?;
    match systemctl(&["stop", &unit]) {
        Ok(()) => Ok(()),
        Err(error) if missing_unit(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<(), WorkerError> {
    let unit = unit_name(id)?;
    if enabled {
        install_unit()?;
        systemctl(&["enable", &unit])
    } else {
        match systemctl(&["disable", &unit]) {
            Ok(()) => Ok(()),
            Err(error) if missing_unit(&error) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Template unit file present. One personal worker in MVP; uninstall removes it.
pub fn is_installed() -> bool {
    unit_path().map(|path| path.is_file()).unwrap_or(false)
}

pub fn uninstall(id: &str) -> Result<(), WorkerError> {
    let _ = stop(id);
    let _ = set_enabled(id, false);
    if let Ok(path) = unit_path() {
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    let _ = systemctl(&["daemon-reload"]);
    Ok(())
}

pub fn is_active(id: &str) -> Result<UnitState, WorkerError> {
    let unit = unit_name(id)?;
    let output = systemctl_output(&["is-active", &unit])?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(match text.as_str() {
        "active" => UnitState::Active,
        "failed" => UnitState::Failed,
        "activating" | "reloading" => UnitState::Starting,
        "inactive" | "deactivating" => UnitState::Inactive,
        _ if output.status.code() == Some(4) => UnitState::Missing,
        _ => UnitState::Unknown,
    })
}

pub fn is_enabled(id: &str) -> bool {
    let Ok(unit) = unit_name(id) else {
        return false;
    };
    systemctl(&["is-enabled", "--quiet", &unit]).is_ok()
}

pub fn linger_state() -> LingerState {
    let Ok(user) = session_user() else {
        return LingerState::Unknown;
    };
    let output = Command::new("loginctl")
        .args(["show-user", &user, "--property=Linger"])
        .output();
    match output {
        Ok(result) if result.status.success() => {
            let text = String::from_utf8_lossy(&result.stdout);
            if text.contains("Linger=yes") {
                LingerState::Enabled
            } else if text.contains("Linger=no") {
                LingerState::Disabled
            } else {
                LingerState::Unknown
            }
        }
        _ => LingerState::Unknown,
    }
}

pub fn enable_linger() -> Result<(), WorkerError> {
    let user = session_user()?;
    let output = Command::new("loginctl")
        .args(["enable-linger", &user])
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(WorkerError::msg(if stderr.is_empty() {
            "loginctl enable-linger failed. This usually needs a real user session.".to_string()
        } else {
            stderr
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitState {
    Active,
    Starting,
    Inactive,
    Failed,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LingerState {
    Enabled,
    Disabled,
    Unknown,
}

fn session_user() -> Result<String, WorkerError> {
    let user = std::env::var("USER").map_err(|_| WorkerError::msg("$USER is not set"))?;
    if !crate::worker::validate::is_safe_session_user(&user) {
        return Err(WorkerError::msg(
            "Refusing linger for an unsafe $USER value.",
        ));
    }
    Ok(user)
}

fn require_uuid(id: &str) -> Result<(), WorkerError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| WorkerError::msg("worker unit instance must be a UUID"))
}

fn systemctl(args: &[&str]) -> Result<(), WorkerError> {
    let output = systemctl_output(args)?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(WorkerError::msg(if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("systemctl --user {} failed", args.join(" "))
        }))
    }
}

fn systemctl_output(args: &[&str]) -> Result<std::process::Output, WorkerError> {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|error| {
            WorkerError::msg(format!(
                "systemctl --user is not available ({error}). Start/stop needs a real Ubuntu user session."
            ))
        })
}

fn missing_unit(error: &WorkerError) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("not found") || text.contains("could not be found") || text.contains("inactive")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    #[test]
    fn unit_name_is_template_instance() {
        assert_eq!(
            unit_name("8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60").unwrap(),
            "worker-dock@8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60.service"
        );
        assert!(unit_name("../evil").is_err());
    }

    #[test]
    fn unit_file_matches_design() {
        let text = render_unit("/home/dev/.local/bin/worker-dock-run").unwrap();
        assert!(
            text.contains("ExecStart=/home/dev/.local/bin/worker-dock-run --supervised-run %i")
        );
        assert!(text.contains("Restart=on-failure"));
        assert!(text.contains("WantedBy=default.target"));
        assert!(text.contains("NoNewPrivileges=yes"));
        assert!(text.contains("RestrictRealtime=yes"));
        assert!(text.contains("RestrictSUIDSGID=yes"));
        assert!(text.contains("LockPersonality=yes"));
        assert!(text.contains("RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK"));
        assert!(text.contains("enumerates network interfaces"));
        assert!(text.contains("SystemCallArchitectures=native"));
        for forbidden in [
            "CapabilityBoundingSet=",
            "AmbientCapabilities=",
            "ProtectSystem=",
            "ProtectHostname=",
            "ProtectKernelTunables=",
            "ProtectKernelModules=",
            "ProtectControlGroups=",
            "PrivateTmp=",
            "PrivateUsers=",
            "Environment=",
        ] {
            assert!(
                !text.lines().any(|line| line.starts_with(forbidden)),
                "user unit must not set {forbidden}"
            );
        }
        assert!(text.contains("The wrapper reads the keychain"));
        assert!(!text.contains("pool"));
        assert!(render_unit("relative/bin").is_err());
    }

    #[cfg(unix)]
    fn stub_unit_text(exec_start: &str) -> String {
        render_unit("/usr/bin/worker-dock-run")
            .unwrap()
            .lines()
            .map(|line| {
                if line.starts_with("ExecStart=") {
                    format!("ExecStart={exec_start}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    /// Starts the generated hardening under a real user systemd. CI sets
    /// `WORKER_DOCK_REQUIRE_SYSTEMD_USER=1` so a skip becomes a failure.
    /// Windows has no systemctl; skip compiling this check there.
    #[cfg(unix)]
    #[test]
    fn generated_user_unit_starts_under_user_systemd() {
        let required = std::env::var("WORKER_DOCK_REQUIRE_SYSTEMD_USER")
            .ok()
            .is_some_and(|value| value == "1");
        if !user_systemd_available() {
            if required {
                panic!(
                    "user systemd is required (WORKER_DOCK_REQUIRE_SYSTEMD_USER=1) but systemctl --user is not ready"
                );
            }
            return;
        }

        let home = crate::cli::paths::home_dir().expect("home");
        let unit_dir = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("systemd")
            .join("user");
        fs::create_dir_all(&unit_dir).expect("systemd user dir");
        let unit_path = unit_dir.join("worker-dock-unit-check.service");
        fs::write(&unit_path, stub_unit_text("/bin/true")).expect("write stub unit");

        let reload = user_systemctl(&["daemon-reload"]);
        assert!(
            reload.status.success(),
            "daemon-reload failed: {}",
            String::from_utf8_lossy(&reload.stderr)
        );

        let start = user_systemctl(&["start", "worker-dock-unit-check.service"]);
        let show = user_systemctl(&[
            "show",
            "worker-dock-unit-check.service",
            "--property=Result",
            "--property=ExecMainStatus",
            "--property=ExecMainCode",
            "--property=ActiveState",
            "--property=SubState",
            "--property=NRestarts",
        ]);
        let show_text = String::from_utf8_lossy(&show.stdout);
        let start_err = String::from_utf8_lossy(&start.stderr);
        let combined = format!("{start_err}\n{show_text}");

        let _ = user_systemctl(&["stop", "worker-dock-unit-check.service"]);
        let _ = user_systemctl(&["reset-failed", "worker-dock-unit-check.service"]);
        let _ = fs::remove_file(&unit_path);
        let _ = user_systemctl(&["daemon-reload"]);

        assert!(
            !combined.contains("status=218")
                && !combined.contains("CAPABILITIES")
                && !combined.contains("ExecMainStatus=218")
                && !show_text.contains("Result=resources")
                && !show_text.contains("Result=protocol")
                && !show_text.contains("Result=timeout")
                && start.status.success(),
            "user unit failed to start (capability/namespace hardening?): {combined}"
        );
        assert!(
            show_text.contains("Result=success") || show_text.contains("ExecMainStatus=0"),
            "expected a clean stub start, got: {show_text}"
        );
    }

    /// Node's `os.networkInterfaces()` uses getifaddrs, which opens a netlink
    /// socket. The official CLI exits EAFNOSUPPORT when the user unit omits
    /// `AF_NETLINK`. CI sets `WORKER_DOCK_REQUIRE_SYSTEMD_USER=1`.
    #[cfg(unix)]
    #[test]
    fn generated_address_families_allow_node_network_interfaces() {
        let required = std::env::var("WORKER_DOCK_REQUIRE_SYSTEMD_USER")
            .ok()
            .is_some_and(|value| value == "1");
        let unit = render_unit("/usr/bin/worker-dock-run").unwrap();
        let families = restrict_address_families(&unit);
        for family in ["AF_UNIX", "AF_INET", "AF_INET6", "AF_NETLINK"] {
            assert!(
                families.split_whitespace().any(|item| item == family),
                "RestrictAddressFamilies missing {family}: {families}"
            );
        }
        if !user_systemd_available() {
            if required {
                panic!(
                    "user systemd is required (WORKER_DOCK_REQUIRE_SYSTEMD_USER=1) but systemctl --user is not ready"
                );
            }
            return;
        }
        let Some(node) = find_node() else {
            if required {
                panic!(
                    "node is required (WORKER_DOCK_REQUIRE_SYSTEMD_USER=1) to run os.networkInterfaces() under RestrictAddressFamilies"
                );
            }
            return;
        };

        let blocked = run_node_network_interfaces(
            &node,
            "AF_UNIX AF_INET AF_INET6",
            &format!("worker-dock-afcheck-deny-{}", std::process::id()),
        );
        let blocked_text = output_text(&blocked);
        assert!(
            !blocked.status.success(),
            "address families without AF_NETLINK unexpectedly allowed os.networkInterfaces(); sandbox was not applied: {blocked_text}"
        );
        assert!(
            blocked_text.contains("uv_interface_addresses")
                || blocked_text.contains("EAFNOSUPPORT")
                || blocked_text.contains("error 97"),
            "expected getifaddrs EAFNOSUPPORT without AF_NETLINK, got: {blocked_text}"
        );

        let allowed = run_node_network_interfaces(
            &node,
            families,
            &format!("worker-dock-afcheck-allow-{}", std::process::id()),
        );
        let allowed_text = output_text(&allowed);
        assert!(
            allowed.status.success(),
            "os.networkInterfaces() failed under RestrictAddressFamilies={families}: {allowed_text}"
        );
    }

    #[cfg(unix)]
    fn restrict_address_families(unit: &str) -> &str {
        unit.lines()
            .find_map(|line| line.strip_prefix("RestrictAddressFamilies="))
            .expect("generated unit sets RestrictAddressFamilies")
    }

    #[cfg(unix)]
    fn find_node() -> Option<PathBuf> {
        let path_var = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path_var) {
            for name in ["node", "nodejs"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    #[cfg(unix)]
    fn output_text(output: &std::process::Output) -> String {
        format!(
            "status={:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    #[cfg(unix)]
    fn run_node_network_interfaces(
        node: &std::path::Path,
        families: &str,
        unit: &str,
    ) -> std::process::Output {
        // Do not print interface addresses; success is an empty exit 0.
        let script = "const nics=require('os').networkInterfaces();if(!nics||Object.keys(nics).length<1)process.exit(2);";
        let args = vec![
            "--wait".to_string(),
            "--pipe".to_string(),
            "--collect".to_string(),
            format!("--unit={unit}"),
            format!("--property=RestrictAddressFamilies={families}"),
            node.display().to_string(),
            "-e".to_string(),
            script.to_string(),
        ];
        user_systemd_run(&args)
    }

    #[cfg(unix)]
    fn user_systemd_run(args: &[String]) -> std::process::Output {
        let direct = systemd_run_output(&["--user".to_string()], args);
        if let Ok(output) = direct {
            if output.status.success()
                || output.status.code().is_some() && !looks_like_missing_bus(&output)
            {
                return output;
            }
        }
        let user = std::env::var("USER").unwrap_or_default();
        let machine = format!("--machine={user}@");
        systemd_run_output(&["--user".to_string(), machine], args)
            .unwrap_or_else(|error| panic!("systemd-run --user unavailable: {error}"))
    }

    #[cfg(unix)]
    fn systemd_run_output(
        prefix: &[String],
        args: &[String],
    ) -> std::io::Result<std::process::Output> {
        let mut command = Command::new("timeout");
        command.arg("30").arg("systemd-run");
        command.args(prefix);
        command.args(args);
        command.env("SYSTEMD_COLORS", "0");
        command.env("SYSTEMD_PAGER", "");
        command.output()
    }

    #[cfg(unix)]
    fn user_systemd_available() -> bool {
        user_systemctl(&["is-system-running"]).status.code().is_some()
            && (std::env::var("XDG_RUNTIME_DIR").ok().is_some_and(|dir| {
                PathBuf::from(dir).join("bus").exists()
            }) || user_systemctl(&["show", "--property=Version"])
                .status
                .success())
    }

    #[cfg(unix)]
    fn user_systemctl(args: &[&str]) -> std::process::Output {
        let direct = Command::new("systemctl")
            .arg("--user")
            .args(args)
            .output();
        if let Ok(output) = direct {
            if output.status.success()
                || output.status.code().is_some() && !looks_like_missing_bus(&output)
            {
                return output;
            }
        }
        let user = std::env::var("USER").unwrap_or_default();
        let machine = format!("{user}@");
        Command::new("systemctl")
            .args(["--user", &format!("--machine={machine}")])
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("systemctl --user unavailable: {error}"))
    }

    #[cfg(unix)]
    fn looks_like_missing_bus(output: &std::process::Output) -> bool {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
        .to_ascii_lowercase();
        text.contains("failed to connect")
            || text.contains("no such file")
            || text.contains("dbus")
    }

    #[test]
    fn linger_rejects_unsafe_user() {
        assert!(!crate::worker::validate::is_safe_session_user(""));
        assert!(!crate::worker::validate::is_safe_session_user("alice;reboot"));
        assert!(crate::worker::validate::is_safe_session_user("alice"));
    }
}
