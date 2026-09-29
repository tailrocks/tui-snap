use super::regex::MiniRegex;
use super::{LocateError, Span, StyleQuery, TextMode};

/// Query over a [`Screen`](crate::screen::Screen) at a revision. Built with `text`/`regex`/`style`/
/// `region`, refined with match modes, combined with `within`/`before`/`after`
/// /`nth`/`first`/`last`/`and`/`or`/`filter`.
///
/// Resolution order is total and documented: scrollback matches (oldest line
/// first) come before viewport matches (row-major), so `first`/`nth`/actions
/// are deterministic.
#[derive(Debug, Clone)]
pub struct Locator {
    pub(crate) kind: LocatorKind,
    /// Text/regex: join wrapped rows into logical lines (default true).
    pub(crate) join_wrapped: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum LocatorKind {
    Text {
        pattern: String,
        mode: TextMode,
    },
    Regex {
        source: String,
        re: MiniRegex,
    },
    Style {
        query: StyleQuery,
    },
    Region {
        x: u16,
        y: u16,
        cols: u16,
        rows: u16,
    },
    Within {
        scope: Box<Locator>,
        inner: Box<Locator>,
    },
    Before {
        main: Box<Locator>,
        anchor: Box<Locator>,
    },
    After {
        main: Box<Locator>,
        anchor: Box<Locator>,
    },
    Nth {
        inner: Box<Locator>,
        index: usize,
    },
    First(Box<Locator>),
    Last(Box<Locator>),
    And {
        a: Box<Locator>,
        b: Box<Locator>,
    },
    Or {
        a: Box<Locator>,
        b: Box<Locator>,
    },
    Filter {
        inner: Box<Locator>,
        pred: fn(&Span) -> bool,
    },
}

impl Locator {
    /// Substring text query (default [`TextMode::Substring`]); refine with
    /// [`Locator::mode`]. Empty patterns and patterns containing `'\n'` fail
    /// at resolve time with [`LocateError::Usage`].
    #[must_use]
    pub fn text(pattern: impl Into<String>) -> Self {
        Self {
            kind: LocatorKind::Text {
                pattern: pattern.into(),
                mode: TextMode::Substring,
            },
            join_wrapped: true,
        }
    }

    /// Tiny-regex query (see module docs for the supported subset).
    /// Unsupported constructs fail here with [`LocateError::Usage`].
    pub fn regex(pattern: &str) -> Result<Self, LocateError> {
        Ok(Self {
            kind: LocatorKind::Regex {
                source: pattern.to_string(),
                re: MiniRegex::parse(pattern, false)?,
            },
            join_wrapped: true,
        })
    }

    /// Case-insensitive tiny-regex query. Non-ASCII haystacks fail at resolve
    /// time with [`LocateError::Unsupported`] (first-char lowering only).
    pub fn regex_case_insensitive(pattern: &str) -> Result<Self, LocateError> {
        Ok(Self {
            kind: LocatorKind::Regex {
                source: pattern.to_string(),
                re: MiniRegex::parse(pattern, true)?,
            },
            join_wrapped: true,
        })
    }

    /// Cell-style query: maximal per-row runs of [`StyleQuery`]-matching lead
    /// cells (styled blanks included, continuations skipped).
    #[must_use]
    pub fn style(query: StyleQuery) -> Self {
        Self {
            kind: LocatorKind::Style { query },
            join_wrapped: true,
        }
    }

    /// Rectangular scope `(x, y, cols, rows)` in grid-local cells: resolves to
    /// one span per covered row. Out-of-bounds rects fail at resolve time
    /// with [`LocateError::Usage`]. Wide cells straddling the edges fail the
    /// same way (never silently cut, Q10).
    #[must_use]
    pub fn region(x: u16, y: u16, cols: u16, rows: u16) -> Self {
        Self {
            kind: LocatorKind::Region { x, y, cols, rows },
            join_wrapped: true,
        }
    }

    /// Set the text match mode (only affects [`Locator::text`]).
    #[must_use]
    pub fn mode(mut self, mode: TextMode) -> Self {
        if let LocatorKind::Text { mode: m, .. } = &mut self.kind {
            *m = mode;
        }
        self
    }

    /// Match physical rows only: disable wrapped-line joining for this
    /// locator (applies to text/regex leaves; combinators inherit per-leaf).
    #[must_use]
    pub fn physical_rows(mut self) -> Self {
        self.join_wrapped = false;
        self
    }

    /// Matches of `inner` fully contained in some match of `scope`
    /// (same line, column range inside; scrollback/viewport areas segregate).
    #[must_use]
    pub fn within(scope: Locator, inner: Locator) -> Self {
        Self::combinator(LocatorKind::Within {
            scope: Box::new(scope),
            inner: Box::new(inner),
        })
    }

    /// Matches of `main` starting before some match of `anchor` (total order:
    /// scrollback oldest-first, then viewport row-major).
    #[must_use]
    pub fn before(main: Locator, anchor: Locator) -> Self {
        Self::combinator(LocatorKind::Before {
            main: Box::new(main),
            anchor: Box::new(anchor),
        })
    }

    /// Matches of `main` starting after some match of `anchor`.
    #[must_use]
    pub fn after(main: Locator, anchor: Locator) -> Self {
        Self::combinator(LocatorKind::After {
            main: Box::new(main),
            anchor: Box::new(anchor),
        })
    }

    /// The `index`-th match in resolution order (0-based). Out of range
    /// resolves to no matches.
    #[must_use]
    pub fn nth(inner: Locator, index: usize) -> Self {
        Self::combinator(LocatorKind::Nth {
            inner: Box::new(inner),
            index,
        })
    }

    /// The first match in resolution order.
    #[must_use]
    pub fn first(inner: Locator) -> Self {
        Self::combinator(LocatorKind::First(Box::new(inner)))
    }

    /// The last match in resolution order.
    #[must_use]
    pub fn last(inner: Locator) -> Self {
        Self::combinator(LocatorKind::Last(Box::new(inner)))
    }

    /// Matches of `a` overlapping some match of `b` (same line, column
    /// ranges intersect). Returns the `a` spans.
    #[must_use]
    pub fn and(a: Locator, b: Locator) -> Self {
        Self::combinator(LocatorKind::And {
            a: Box::new(a),
            b: Box::new(b),
        })
    }

    /// Union of both match sets, deduplicated, in resolution order.
    #[must_use]
    pub fn or(a: Locator, b: Locator) -> Self {
        Self::combinator(LocatorKind::Or {
            a: Box::new(a),
            b: Box::new(b),
        })
    }

    /// Keep matches satisfying `pred`.
    #[must_use]
    pub fn filter(inner: Locator, pred: fn(&Span) -> bool) -> Self {
        Self::combinator(LocatorKind::Filter {
            inner: Box::new(inner),
            pred,
        })
    }

    fn combinator(kind: LocatorKind) -> Self {
        Self {
            kind,
            join_wrapped: true,
        }
    }
}
