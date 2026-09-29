use serde::{Deserialize, Serialize};

/// Where a frame came from. Recorded so reports are auditable; `created_unix`
/// is informational only and excluded from equality comparisons that must be
/// deterministic (use [`Frame::digest`](crate::frame::Frame::digest) / cell comparison for gates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Capturing tool name.
    pub tool: String,
    /// Capturing tool version.
    pub tool_version: String,
    /// Active profile name.
    pub profile: String,
    /// Capture source (view path, session id, ...).
    pub source: String,
    /// Invoked command line.
    pub argv: Vec<String>,
    /// Creation time as Unix seconds (informational only).
    pub created_unix: u64,
}

impl Provenance {
    /// Provenance stamped with the current Unix time (0 when the clock fails).
    #[must_use]
    pub fn now(profile: &str, source: &str, argv: Vec<String>) -> Self {
        Self {
            tool: "tuisnap".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            profile: profile.to_string(),
            source: source.to_string(),
            argv,
            created_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        }
    }
}
