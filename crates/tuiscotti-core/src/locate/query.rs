use crate::frame::{Cell, Color, UnderlineStyle};

/// How [`Locator::text`](crate::locate::Locator::text) interprets its pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextMode {
    /// Pattern appears anywhere in the (logical) row text.
    #[default]
    Substring,
    /// Whole trimmed row text equals the pattern.
    Exact,
    /// Substring after ASCII-oriented case folding (see module docs).
    CaseInsensitive,
    /// Substring after collapsing every whitespace run to one space.
    Normalized,
}

/// Cell-style predicate for [`Locator::style`](crate::locate::Locator::style). Every specified field must
/// match; unspecified fields are ignored. Continuation cells are never
/// matched directly (their lead carries the style); styled blanks ARE matched.
#[derive(Debug, Clone, Copy, Default)]
pub struct StyleQuery {
    fg: Option<Color>,
    bg: Option<Color>,
    bold: Option<bool>,
    dim: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    underline_style: Option<UnderlineStyle>,
    underline_color: Option<Color>,
    strikethrough: Option<bool>,
    reverse: Option<bool>,
    hidden: Option<bool>,
    blink: Option<bool>,
    custom: Option<fn(&Cell) -> bool>,
}

impl StyleQuery {
    /// Empty query: matches every lead cell until constrained.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Require this foreground color.
    #[must_use]
    pub fn fg(mut self, c: Color) -> Self {
        self.fg = Some(c);
        self
    }

    /// Require this background color.
    #[must_use]
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = Some(c);
        self
    }

    /// Require bold (`true`) or non-bold (`false`).
    #[must_use]
    pub fn bold(mut self, v: bool) -> Self {
        self.bold = Some(v);
        self
    }

    /// Require dim (`true`) or non-dim (`false`).
    #[must_use]
    pub fn dim(mut self, v: bool) -> Self {
        self.dim = Some(v);
        self
    }

    /// Require italic (`true`) or non-italic (`false`).
    #[must_use]
    pub fn italic(mut self, v: bool) -> Self {
        self.italic = Some(v);
        self
    }

    /// Any (`true`) or no (`false`) underline, regardless of style.
    #[must_use]
    pub fn underline(mut self, v: bool) -> Self {
        self.underline = Some(v);
        self
    }

    /// Exact underline style.
    #[must_use]
    pub fn underline_style(mut self, s: UnderlineStyle) -> Self {
        self.underline_style = Some(s);
        self
    }

    /// Exact underline (SGR 58) color.
    #[must_use]
    pub fn underline_color(mut self, c: Color) -> Self {
        self.underline_color = Some(c);
        self
    }

    /// Require strikethrough (`true`) or not (`false`).
    #[must_use]
    pub fn strikethrough(mut self, v: bool) -> Self {
        self.strikethrough = Some(v);
        self
    }

    /// Require reverse video (`true`) or not (`false`).
    #[must_use]
    pub fn reverse(mut self, v: bool) -> Self {
        self.reverse = Some(v);
        self
    }

    /// Require concealed (`true`) or visible (`false`) cells.
    #[must_use]
    pub fn hidden(mut self, v: bool) -> Self {
        self.hidden = Some(v);
        self
    }

    /// Require blinking (`true`) or steady (`false`) cells.
    #[must_use]
    pub fn blink(mut self, v: bool) -> Self {
        self.blink = Some(v);
        self
    }

    /// Extra caller predicate, combined with the field constraints.
    #[must_use]
    pub fn custom(mut self, p: fn(&Cell) -> bool) -> Self {
        self.custom = Some(p);
        self
    }

    pub(crate) fn matches(&self, cell: &Cell) -> bool {
        if cell.continuation {
            return false;
        }
        if let Some(fg) = self.fg
            && cell.fg != fg
        {
            return false;
        }
        if let Some(bg) = self.bg
            && cell.bg != bg
        {
            return false;
        }
        let m = cell.mods;
        for (want, got) in [
            (self.bold, m.bold),
            (self.dim, m.dim),
            (self.italic, m.italic),
            (self.underline, m.underline),
            (self.strikethrough, m.strikethrough),
            (self.reverse, m.reverse),
            (self.hidden, m.hidden),
            (self.blink, m.blink),
        ] {
            if let Some(w) = want
                && w != got
            {
                return false;
            }
        }
        if let Some(s) = self.underline_style
            && cell.mods.effective_underline_style() != s
        {
            return false;
        }
        if let Some(uc) = self.underline_color
            && cell.underline_color != uc
        {
            return false;
        }
        if let Some(p) = self.custom
            && !p(cell)
        {
            return false;
        }
        true
    }
}
