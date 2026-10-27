pub(crate) mod config;
pub(crate) mod detect;
pub(crate) mod error;
mod install;
pub(crate) mod paths;
mod version;

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use config::CliConfig;
use detect::DetectedCli;
use error::CliError;
use install::Progress;
use paths::{host_arch, host_os, official_archive_url, official_install_script_url};
use version::{is_vetted, newest_vetted, PINNED_VERSION};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub present: bool,
    pub path: Option<String>,
    pub installed_version: Option<String>,
    pub pinned_version: String,
    pub official_latest: Option<String>,
    pub on_pinned: bool,
    pub update_available: bool,
    pub official_newer_unvetted: bool,
    pub auto_update_disabled: bool,
    pub auto_update_flag_supported: bool,
    pub archive_url: String,
    pub install_script_url: String,
}

#[tauri::command]
pub async fn cli_status() -> Result<CliStatus, CliError> {
    status().await
}

#[tauri::command]
pub async fn cli_install(app: AppHandle) -> Result<CliStatus, CliError> {
    apply_vetted_version(&app, PINNED_VERSION).await
}

#[tauri::command]
pub async fn cli_update(app: AppHandle) -> Result<CliStatus, CliError> {
    apply_vetted_version(&app, newest_vetted()).await
}

pub async fn status() -> Result<CliStatus, CliError> {
    let config = config::load()?;
    let detected = detect::detect(&config).await?;
    let mut official_latest = install::fetch_official_latest().await.ok().flatten();
    if official_latest.is_none() {
        if let Some(found) = detected.as_ref() {
            if let Ok(about) = detect::about(&found.path, found.auto_update_flag_supported).await {
                official_latest = about.latest_version;
            }
        }
    }
    Ok(build_status(&config, detected.as_ref(), official_latest))
}

fn build_status(
    config: &CliConfig,
    detected: Option<&DetectedCli>,
    official_latest: Option<String>,
) -> CliStatus {
    let pinned = if version::is_cli_version(&config.pinned_version) {
        config.pinned_version.as_str()
    } else {
        PINNED_VERSION
    };
    let installed = detected.map(|item| item.version.as_str());
    let on_pinned = installed == Some(pinned);
    let update_available = installed.is_some_and(|version| version != newest_vetted());
    let official_newer_unvetted = official_latest
        .as_deref()
        .is_some_and(|latest| latest != pinned && !is_vetted(latest));
    let archive_url = official_archive_url(pinned, host_os(), host_arch().unwrap_or("x64"))
        .map(|url| url.to_string())
        .unwrap_or_default();
    let install_script_url = official_install_script_url(host_os())
        .map(|url| url.to_string())
        .unwrap_or_else(|_| "https://cursor.com/install".to_string());
    CliStatus {
        present: detected.is_some(),
        path: detected.map(|item| item.path.display().to_string()),
        installed_version: installed.map(str::to_string),
        pinned_version: pinned.to_string(),
        official_latest,
        on_pinned,
        update_available,
        official_newer_unvetted,
        auto_update_disabled: config.disable_auto_update
            && detected
                .map(|item| item.auto_update_flag_supported)
                .unwrap_or(true),
        auto_update_flag_supported: detected
            .map(|item| item.auto_update_flag_supported)
            .unwrap_or(true),
        archive_url,
        install_script_url,
    }
}

async fn apply_vetted_version(app: &AppHandle, version: &str) -> Result<CliStatus, CliError> {
    if !is_vetted(version) {
        return Err(CliError::msg(format!(
            "{version} is not a vetted official CLI version"
        )));
    }
    emit(
        app,
        "preparing",
        format!("Install official agent CLI {version}"),
    );
    let archive = install::download_archive(version, |progress| {
        let _ = app.emit("cli-progress", progress);
    })
    .await?;
    emit(app, "extracting", archive.display().to_string());
    let version_dir = tokio::task::spawn_blocking({
        let archive = archive.clone();
        let version = version.to_string();
        move || install::extract_archive(&archive, &version)
    })
    .await
    .map_err(|error| CliError::msg(format!("extract task failed: {error}")))??;
    emit(app, "linking", version_dir.display().to_string());
    let shim = install::link_shims(&version_dir)?;
    emit(app, "verifying", shim.display().to_string());
    let detected = detect::probe(&shim).await?;
    if detected.version != version {
        return Err(CliError::msg(format!(
            "installed {} but expected {version}",
            detected.version
        )));
    }
    if !detected.auto_update_flag_supported {
        return Err(CliError::msg(
            "this official CLI build does not accept --disable-auto-update",
        ));
    }
    let resolved = canonicalize_or(&detected.path);
    let mut config = config::load()?;
    config.path = resolved.display().to_string();
    config.install_method = "official_archive".to_string();
    config.pinned_version = version.to_string();
    config.disable_auto_update = true;
    config::save_cli_fields(&config)?;
    detect::invalidate_detect_cache();
    detect::remember_detected(Some(detected.clone()));
    emit(app, "done", detected.version.clone());
    let official_latest = install::fetch_official_latest().await.ok().flatten();
    Ok(build_status(&config, Some(&detected), official_latest))
}

fn emit(app: &AppHandle, phase: &str, detail: impl Into<String>) {
    let _ = app.emit(
        "cli-progress",
        Progress {
            phase: phase.to_string(),
            detail: detail.into(),
        },
    );
}

fn canonicalize_or(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_cli_offers_install_not_update() {
        let status = build_status(&CliConfig::default(), None, None);
        assert!(!status.present);
        assert!(!status.update_available);
        assert_eq!(status.pinned_version, PINNED_VERSION);
        assert!(status.auto_update_disabled);
    }

    #[test]
    fn other_installed_version_offers_vetted_update() {
        let detected = DetectedCli {
            path: PathBuf::from("/tmp/agent"),
            version: "2026.01.01-aaaaaaa".into(),
            auto_update_flag_supported: true,
        };
        let status = build_status(&CliConfig::default(), Some(&detected), None);
        assert!(status.present);
        assert!(status.update_available);
        assert!(!status.on_pinned);
    }

    #[test]
    fn unvetted_official_latest_is_called_out() {
        let detected = DetectedCli {
            path: PathBuf::from("/tmp/agent"),
            version: PINNED_VERSION.into(),
            auto_update_flag_supported: true,
        };
        let status = build_status(
            &CliConfig::default(),
            Some(&detected),
            Some("2026.12.01-fffffff".into()),
        );
        assert!(status.on_pinned);
        assert!(!status.update_available);
        assert!(status.official_newer_unvetted);
    }
}
