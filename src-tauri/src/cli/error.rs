use std::path::PathBuf;

/// Recoverable CLI lifecycle failure. Messages are safe to show in the UI.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Message(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("official CLI at {path} exited {status}: {stderr}")]
    CommandFailed {
        path: PathBuf,
        status: i32,
        stderr: String,
    },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl CliError {
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}

impl From<reqwest::Error> for CliError {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(crate::redact::redact(&error.to_string()))
    }
}

impl From<url::ParseError> for CliError {
    fn from(error: url::ParseError) -> Self {
        Self::msg(format!("invalid URL: {error}"))
    }
}

impl serde::Serialize for CliError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Replace an exact secret with `***` after generic redaction.
/// Never log the unredacted string.
pub fn redact_secret(text: &str, secret: Option<&str>) -> String {
    crate::redact::redact_with_secret(text, secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_exact_secret_only() {
        assert_eq!(
            redact_secret(
                "token=abcd-known-secret and more",
                Some("abcd-known-secret")
            ),
            "token=*** and more"
        );
        assert_eq!(redact_secret("unchanged", None), "unchanged");
        assert_eq!(redact_secret("unchanged", Some("")), "unchanged");
    }
}
