use serde::{Deserialize, Serialize};

/// Where a frame came from. Recorded so reports are auditable; `created_unix`
/// is informational only and excluded from equality comparisons that must be
/// deterministic (use [`Frame::digest`](crate::frame::Frame::digest) / cell comparison for gates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub tool: String,
    pub tool_version: String,
    pub profile: String,
    pub source: String,
    pub argv: Vec<String>,
    pub created_unix: u64,
}

impl Provenance {
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
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}
