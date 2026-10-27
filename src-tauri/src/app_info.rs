//! App identity used by Settings, the tray tooltip, and outbound HTTP.

pub const NAME: &str = "worker-dock";

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn user_agent() -> String {
    format!("{NAME}/{} (Stand Easy Ops)", version())
}

#[tauri::command]
pub fn app_version() -> String {
    version().to_string()
}

#[cfg(test)]
const MSI_FIELD_MAX: u32 = 65535;

/// Windows WiX MajorUpgrade compares `major.minor.build` only.
#[cfg(test)]
fn is_msi_product_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let Some(major) = parse_field(parts.next()) else {
        return false;
    };
    let Some(minor) = parse_field(parts.next()) else {
        return false;
    };
    let Some(build) = parse_field(parts.next()) else {
        return false;
    };
    parts.next().is_none()
        && major <= MSI_FIELD_MAX
        && minor <= MSI_FIELD_MAX
        && build <= MSI_FIELD_MAX
}

#[cfg(test)]
fn parse_field(part: Option<&str>) -> Option<u32> {
    let part = part?;
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_version_is_msi_safe() {
        assert!(
            is_msi_product_version(version()),
            "CARGO_PKG_VERSION {} is not major.minor.build with fields <= {MSI_FIELD_MAX}",
            version()
        );
    }

    #[test]
    fn accepts_ci_run_number_scheme() {
        assert!(is_msi_product_version("0.1.1"));
        assert!(is_msi_product_version("0.1.65535"));
    }

    #[test]
    fn rejects_invalid_msi_versions() {
        for value in [
            "0.1",
            "0.1.2.3",
            "v0.1.2",
            "0.1.65536",
            "latest",
            "0.1.0-beta",
        ] {
            assert!(!is_msi_product_version(value), "{value}");
        }
    }

    #[test]
    fn user_agent_includes_crate_version() {
        assert_eq!(
            user_agent(),
            format!("worker-dock/{} (Stand Easy Ops)", version())
        );
    }
}
