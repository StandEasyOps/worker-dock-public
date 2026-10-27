/// Official CLI builds worker-dock has gold-filed and will install.
/// Newest vetted entry is the pin the UI installs and updates to.
pub const VETTED_VERSIONS: &[&str] = &["2026.10.19-6bbf2c4"];

/// Pinned SHA-256 of each official archive we will extract. `(version, os, arch, hex)`.
/// Bump together with `VETTED_VERSIONS` after hashing a freshly downloaded official package.
pub const ARCHIVE_SHA256: &[(&str, &str, &str, &str)] = &[
    (
        "2026.10.19-6bbf2c4",
        "linux",
        "x64",
        "cf80cc18d8b33bf33e762353977ca1868cfc45290c92c0ffdcef83bb8b37b44f",
    ),
    (
        "2026.10.19-6bbf2c4",
        "linux",
        "arm64",
        "71a90f12a8b8183a71442b7ed4037bddc1a0f88114ec4fb9ec92782acb8e7d84",
    ),
    (
        "2026.10.19-6bbf2c4",
        "windows",
        "x64",
        "518e123c39ee7765b11b677a6d5716fe158209cb7fee8a203a7dd940848f050b",
    ),
    (
        "2026.10.19-6bbf2c4",
        "windows",
        "arm64",
        "a13778e54215944cf7a69b38d08dd5c5d2cde21e9c3e73de8f65943cb286d00c",
    ),
];

pub fn expected_archive_sha256(version: &str, os: &str, arch: &str) -> Option<&'static str> {
    ARCHIVE_SHA256
        .iter()
        .find(|(pinned, pinned_os, pinned_arch, _)| {
            *pinned == version && *pinned_os == os && *pinned_arch == arch
        })
        .map(|(_, _, _, digest)| *digest)
}

/// Currently supported official `agent` build (spike + live install script, 2026-10-20).
pub const PINNED_VERSION: &str = VETTED_VERSIONS[VETTED_VERSIONS.len() - 1];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliVersion(String);

impl CliVersion {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        if !is_cli_version(trimmed) {
            return Err(format!("not an official agent CLI version: {raw}"));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CliVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub fn is_cli_version(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 18 {
        return false;
    }
    // YYYY.MM.DD-<githash>
    let (date, hash) = match value.split_once('-') {
        Some(parts) => parts,
        None => return false,
    };
    let date_ok = date.len() == 10
        && date.as_bytes()[4] == b'.'
        && date.as_bytes()[7] == b'.'
        && date.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7) || byte.is_ascii_digit()
        });
    date_ok && hash.len() >= 7 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn is_vetted(version: &str) -> bool {
    VETTED_VERSIONS.contains(&version)
}

pub fn newest_vetted() -> &'static str {
    PINNED_VERSION
}

/// Pull the first `YYYY.MM.DD-<hash>` from official installer text. Do not execute the script.
pub fn version_from_installer_text(text: &str) -> Option<String> {
    for token in text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.' && ch != '-') {
        if is_cli_version(token) {
            return Some(token.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_spike_version() {
        assert!(is_cli_version("2026.10.19-6bbf2c4"));
        assert_eq!(
            CliVersion::parse(" 2026.10.19-6bbf2c4\n").unwrap().as_str(),
            "2026.10.19-6bbf2c4"
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(!is_cli_version("latest"));
        assert!(!is_cli_version("1.2.3"));
        assert!(!is_cli_version("2026.10.19"));
        assert!(!is_cli_version("../2026.10.19-6bbf2c4"));
        assert!(CliVersion::parse("agent").is_err());
    }

    #[test]
    fn pin_is_vetted() {
        assert!(is_vetted(PINNED_VERSION));
        assert_eq!(newest_vetted(), "2026.10.19-6bbf2c4");
    }

    #[test]
    fn every_vetted_version_has_archive_digests() {
        for version in VETTED_VERSIONS {
            for os in ["linux", "windows"] {
                for arch in ["x64", "arm64"] {
                    let digest = expected_archive_sha256(version, os, arch)
                        .unwrap_or_else(|| panic!("missing SHA-256 for {version}/{os}/{arch}"));
                    assert_eq!(digest.len(), 64);
                    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
                }
            }
        }
    }

    #[test]
    fn parses_linux_install_script() {
        let script = r#"DOWNLOAD_URL="https://downloads.cursor.com/lab/2026.10.19-6bbf2c4/${OS}/${ARCH}/agent-cli-package.tar.gz""#;
        assert_eq!(
            version_from_installer_text(script).as_deref(),
            Some("2026.10.19-6bbf2c4")
        );
    }

    #[test]
    fn parses_windows_install_script() {
        let script = "$version = '2026.10.19-6bbf2c4'\r\n$downloadUrl = 'https://downloads.cursor.com/lab/2026.10.19-6bbf2c4/'";
        assert_eq!(
            version_from_installer_text(script).as_deref(),
            Some("2026.10.19-6bbf2c4")
        );
    }
}
