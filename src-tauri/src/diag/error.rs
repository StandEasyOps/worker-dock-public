/// Recoverable diagnostics failure. Messages are safe to show in the UI.
#[derive(Debug, thiserror::Error)]
pub enum DiagError {
    #[error("{0}")]
    Message(String),
}

impl DiagError {
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}

impl serde::Serialize for DiagError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<crate::cli::error::CliError> for DiagError {
    fn from(error: crate::cli::error::CliError) -> Self {
        Self::msg(error.to_string())
    }
}

impl From<crate::worker::error::WorkerError> for DiagError {
    fn from(error: crate::worker::error::WorkerError) -> Self {
        Self::msg(error.to_string())
    }
}
