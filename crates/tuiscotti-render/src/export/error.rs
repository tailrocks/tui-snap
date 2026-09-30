//! Export failure type: [`ExportError`].

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Export failure: explicit, never silent.
#[derive(Debug)]
pub enum ExportError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// Encoder failure (internal encoder or external `ffmpeg`).
    Encode(String),
    /// Caller input rejected (empty frames, length mismatch, bad dims, ...).
    InvalidInput(String),
    /// The external encoder binary is not installed.
    EncoderMissing {
        /// Binary name (`ffmpeg`).
        tool: &'static str,
        /// Where/how to install it.
        install: &'static str,
    },
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Io(e) => write!(f, "export I/O error: {e}"),
            ExportError::Encode(e) => write!(f, "export encode error: {e}"),
            ExportError::InvalidInput(e) => write!(f, "export invalid input: {e}"),
            ExportError::EncoderMissing { tool, install } => {
                write!(f, "export encoder missing: `{tool}` not found ({install})")
            }
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExportError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        ExportError::Io(e)
    }
}

impl ExportError {
    /// True only for the missing-external-encoder path.
    #[must_use]
    pub fn is_encoder_missing(&self) -> bool {
        matches!(self, ExportError::EncoderMissing { .. })
    }
}
