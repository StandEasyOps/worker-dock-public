use crate::cli::error::CliError;

/// Recoverable sign-in failure. Messages are safe to show in the UI and must
/// never include a personal API key or keychain secret bytes.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Cli(#[from] CliError),
}

impl AuthError {
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }

    pub fn cancelled() -> Self {
        Self::Message("Browser sign-in was cancelled.".into())
    }
}

impl serde::Serialize for AuthError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
