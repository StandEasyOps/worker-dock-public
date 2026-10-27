//! Product name for the window, tray tooltip, and About.

pub const DISPLAY_NAME: &str = "worker-dock";

pub fn tray_tooltip(headline: &str, reason: &str) -> String {
    let version = crate::app_info::version();
    if reason.is_empty() {
        format!("{DISPLAY_NAME} {version} · {headline}")
    } else {
        format!("{DISPLAY_NAME} {version} · {headline} — {reason}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_is_worker_dock() {
        assert_eq!(DISPLAY_NAME, "worker-dock");
        assert!(!DISPLAY_NAME.to_ascii_lowercase().contains("cursor"));
    }

    #[test]
    fn tooltip_includes_display_name_and_headline() {
        let version = crate::app_info::version();
        assert_eq!(
            tray_tooltip("Not ready", ""),
            format!("worker-dock {version} · Not ready")
        );
        assert_eq!(
            tray_tooltip("Ready", "service running"),
            format!("worker-dock {version} · Ready — service running")
        );
    }
}
