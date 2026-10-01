/// Import/validation failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError(pub String);

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid frame: {}", self.0)
    }
}

impl std::error::Error for FrameError {}
