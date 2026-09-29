//! Strict/perceptual gate types: verdicts, policies, errors.

/// Pixel-gate failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffError(pub String);

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pixel diff error: {}", self.0)
    }
}

impl std::error::Error for DiffError {}

/// How the strict gate treats the alpha channel. Explicit by construction:
/// alpha is never silently dropped (C03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaPolicy {
    /// Exact RGBA byte identity: dimensions plus every R, G, B **and A**
    /// channel must match. Default. RGB inputs decode with uniform A=255,
    /// so RGB-vs-RGB behaves as RGB-exact.
    #[default]
    StraightRgba,
    /// Both images must be fully opaque (every alpha exactly 255) **and**
    /// RGB-identical. A non-255 alpha anywhere fails the gate: callers that
    /// know their pipeline is opaque assert that here instead of ignoring
    /// the channel.
    Opaque,
}

/// Outcome of one PNG-vs-PNG comparison.
#[derive(Debug)]
pub struct PixelVerdict {
    /// True when both inputs decoded to the same dimensions.
    pub dims_equal: bool,
    /// Decoded `(width, height)` of the expected input.
    pub expected_dims: (u32, u32),
    /// Decoded `(width, height)` of the actual input.
    pub actual_dims: (u32, u32),
    /// THE strict verdict: dimensions equal AND decoded pixels identical
    /// under [`Self::alpha_policy`]. This bit — never [`Self::score`] —
    /// establishes strict equality.
    pub pixels_equal: bool,
    /// The alpha policy this verdict was computed under.
    pub alpha_policy: AlphaPolicy,
    /// 1.0 exactly when [`Self::pixels_equal`]; a perceptual diagnostic
    /// capped strictly below 1.0 otherwise (0.0 when dimensions differ).
    /// Invariant: `score >= 1.0` implies decoded-pixel identity, so legacy
    /// `score < threshold` gates stay sound — but new code must branch on
    /// [`Self::pixels_equal`].
    pub score: f64,
    /// Red-overlay diff image (empty when pixels are equal or dimensions
    /// differ).
    pub diff_png: Vec<u8>,
}

/// Explicit perceptual gate: a similarity threshold in `[0.0, 1.0]`,
/// validated at construction. This is the ONLY place a float threshold
/// exists — the strict gate takes none. Meeting the threshold means
/// "similar enough for review", never "pixel-equal": a perceptual policy
/// cannot report exact equality (C04).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerceptualPolicy {
    threshold: f64,
}

impl PerceptualPolicy {
    /// Validated constructor: rejects NaN, infinities, and anything outside
    /// `[0.0, 1.0]`. An invalid tolerance is an error, never silently
    /// applied (a NaN threshold would make every `score < threshold`
    /// comparison false and fake a match).
    ///
    /// # Errors
    ///
    /// Returns `DiffError` when the threshold is non-finite or outside `[0.0, 1.0]`.
    pub fn new(threshold: f64) -> Result<Self, DiffError> {
        if !threshold.is_finite() {
            return Err(DiffError(format!(
                "invalid pixel_threshold {threshold}: must be finite"
            )));
        }
        if !(0.0..=1.0).contains(&threshold) {
            return Err(DiffError(format!(
                "invalid pixel_threshold {threshold}: must be in [0.0, 1.0]"
            )));
        }
        Ok(Self { threshold })
    }

    /// The validated review-bar threshold.
    #[must_use]
    pub fn threshold(self) -> f64 {
        self.threshold
    }

    /// Diagnostic gate only: does this similarity meet the review bar?
    /// Never a claim of pixel equality.
    #[must_use]
    pub fn allows(self, score: f64) -> bool {
        score >= self.threshold
    }
}
