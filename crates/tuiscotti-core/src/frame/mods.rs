use serde::{Deserialize, Serialize};

/// Underline style (SGR 4 / 4:x; kitty/ITU numbering: `4:1` single through
/// `4:5` dashed, `4:0`/`24` cancel). Stored in [`Mods::underline_style`]
/// alongside the legacy [`Mods::underline`] bool; readers use
/// [`Mods::effective_underline_style`] so both agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub enum UnderlineStyle {
    /// No underline (default; omitted from stored JSON and snapshots).
    #[default]
    None,
    /// SGR 4 / 4:1.
    Single,
    /// SGR 4:2.
    Double,
    /// SGR 4:3 (undercurl).
    Curly,
    /// SGR 4:4.
    Dotted,
    /// SGR 4:5.
    Dashed,
}

impl UnderlineStyle {
    /// Any visible underline, regardless of style.
    #[must_use]
    pub fn is_some(self) -> bool {
        !matches!(self, UnderlineStyle::None)
    }

    /// No underline. Used by `skip_serializing_if` so stored frames stay sparse.
    #[must_use]
    pub fn is_none(&self) -> bool {
        matches!(self, UnderlineStyle::None)
    }

    /// Canonical token used by text/JSON projections (`None` has no token;
    /// callers omit it).
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            UnderlineStyle::None => "-",
            UnderlineStyle::Single => "underline",
            UnderlineStyle::Double => "double-underline",
            UnderlineStyle::Curly => "undercurl",
            UnderlineStyle::Dotted => "dotted-underline",
            UnderlineStyle::Dashed => "dashed-underline",
        }
    }
}

/// Cell modifiers retained in canonical data. Blink phase is frozen visible;
/// hidden glyphs are omitted by renderers but their source symbols remain in
/// canonical data. Concealment is not redaction: never capture real secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Mods {
    pub hidden: bool,
    pub blink: bool,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    /// Underline style refinement (SGR 4:x). Additive v3 field: missing in
    /// legacy files (defaults to `None`) and omitted from stored JSON when
    /// `None`, so only new data carries it. Producer invariant:
    /// `underline == underline_style.is_some()`; legacy `underline=true`
    /// cells with `None` read as Single via
    /// [`Mods::effective_underline_style`].
    #[serde(default, skip_serializing_if = "UnderlineStyle::is_none")]
    pub underline_style: UnderlineStyle,
    pub strikethrough: bool,
    pub reverse: bool,
}

impl Mods {
    /// Effective underline style: the explicit style when set, else Single
    /// for legacy `underline=true` cells, else None. All readers (canonical
    /// projections, renderer, SGR dump, assertions) use this so legacy and
    /// new data agree.
    #[must_use]
    pub fn effective_underline_style(self) -> UnderlineStyle {
        if self.underline_style.is_some() {
            self.underline_style
        } else if self.underline {
            UnderlineStyle::Single
        } else {
            UnderlineStyle::None
        }
    }
}
