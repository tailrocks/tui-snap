/// Screen/region/observation construction failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenError(pub String);

impl std::fmt::Display for ScreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid screen: {}", self.0)
    }
}

impl std::error::Error for ScreenError {}

/// Maximum screen dimension (mirrors `frame::MAX_DIM`; static views carry no
/// PTY minimums, so 1x1 and 1-column/1-row screens are valid).
pub const MAX_DIM: u16 = crate::frame::MAX_DIM;
