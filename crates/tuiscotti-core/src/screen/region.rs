use super::Screen;

// ---------------------------------------------------------------------------
// Region
// ---------------------------------------------------------------------------

/// How a [`Region`] treats cells outside its bounds. Recorded in evidence;
/// geometry is always preserved (M07).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionPolicy {
    /// Cells outside the region are clipped away.
    Clip,
    /// Cells outside the region are masked (M1 records the policy; content
    /// masking itself is a later milestone).
    Mask,
}

/// A cropped screen with geometry/origin preserved and its [`RegionPolicy`]
/// recorded. See [`Screen::region`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Region {
    pub(crate) screen: Screen,
    pub(crate) policy: RegionPolicy,
}

impl Region {
    #[must_use]
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    #[must_use]
    pub fn policy(&self) -> RegionPolicy {
        self.policy
    }
}
