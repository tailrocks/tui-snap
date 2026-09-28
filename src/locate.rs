//! M4 locator core (backlog Q01-Q05, Q08, Q10): Playwright-style queries over
//! [`Screen`] plus a revision (`u64`, usually from [`Observation`]).
//!
//! - Builders: [`Locator::text`], [`Locator::regex`], [`Locator::style`],
//!   [`Locator::region`] with documented match modes ([`TextMode`]).
//! - Combinators: [`Locator::within`], `before`/`after`, `nth`/`first`/`last`,
//!   `and`/`or`/`filter`. They operate on terminal cell [`Span`]s in row-major
//!   order (scrollback matches first, oldest to newest, then viewport rows).
//! - [`Span`]s carry viewport coordinates, the screen origin, and revision, and
//!   are wide-cell aware (continuation cells are skipped; a wide lead counts 2
//!   columns). Spans resolved from [`Region::screen`](crate::screen::Region)
//!   keep the region's origin (Q10).
//! - [`Locator::resolve_unique`] fails on 0 matches ([`LocateError::NotFound`])
//!   or 2+ matches ([`LocateError::Ambiguous`], which lists the matches).
//! - Scrollback matches are flagged non-clickable: building an action from one
//!   fails with [`LocateError::ViewportOnly`] (Q03).
//! - Retryable assertions ([`Locator::expect_visible`], `expect_text`,
//!   `expect_count`) poll a caller-supplied observer to ONE deadline; usage and
//!   unsupported errors fail immediately without waiting (Q04).
//! - Temporal assertions: [`Locator::present_now`], `eventually_absent`,
//!   `remains_absent` (Q08).
//! - Actions ([`PendingAction::click`], [`PendingAction::submit`]) re-validate
//!   a unique target immediately before delivery and refuse stale revisions
//!   with [`LocateError::StaleTarget`]: a click is never delivered stale, and
//!   readiness retries never execute the action sink (Q05).
//!
//! ## Matcher limits (std only, no regex crate)
//!
//! - Text modes: substring (default), whole-row exact, ASCII-oriented
//!   case-insensitive folding (`to_lowercase`, first char only; full Unicode
//!   case folding is NOT performed), and whitespace-normalized substring
//!   (runs of whitespace collapse to one space; matches map back to original
//!   columns, so interior-run boundaries are approximate by up to the run
//!   length — start/end columns stay exact).
//! - Regex subset: literals, `.` (one char, never a row boundary), `*`/`+`/`?`
//!   (greedy, backtracking), classes `[abc]`/`[a-z]`/`[^..]`, escapes `\c`,
//!   `\d`/`\w`/`\s` and `\D`/`\W`/`\S`, leading `^` and trailing `$` anchors.
//!   NOT supported: `|` alternation, `()` groups, `{m,n}` repetition,
//!   lookaround, backreferences, multi-line patterns. `^`/`$` are anchors only
//!   in leading/trailing position; elsewhere they are literals. Empty matches
//!   are skipped. A pattern using an unsupported construct fails at build time
//!   with [`LocateError::Usage`].
//! - Wrapped lines: text/regex matching joins consecutive rows into logical
//!   lines when the upper row's last column holds a non-blank cell (the wrap
//!   heuristic). Disable with [`Locator::physical_rows`]. Joined matches
//!   produce one [`Span`] covering the start/end rows. Style/region locators
//!   always work on physical rows.
//! - Row text excludes trailing blank cells; leading/interior blanks are kept.
//!
//! ## Scrollback
//!
//! [`Screen`] carries viewport cells only, so scrollback lines (plain text,
//! oldest first) are supplied per call to
//! [`Locator::resolve_with_scrollback`]. Scrollback spans carry
//! `scrollback_index` and `scrollback: true`. Style/region locators ignore
//! scrollback (it has no cell geometry); text/regex combinators apply to both.

use crate::frame::{Cell, Color};
use crate::screen::{Observation, Screen};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Locator failure. [`LocateError::Usage`] and [`LocateError::Unsupported`]
/// are permanent: retry loops return them immediately without waiting for the
/// deadline. All other variants are retryable observations except
/// [`LocateError::StaleTarget`] and [`LocateError::ViewportOnly`], which are
/// action-delivery refusals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocateError {
    /// Invalid locator construction or arguments (empty text pattern,
    /// bad regex, newline in pattern, out-of-bounds region, ...). Fix the
    /// caller; retrying cannot help.
    Usage(String),
    /// A capability the matcher cannot provide (currently: case-insensitive
    /// regex over non-ASCII text, where first-char lowering is unreliable).
    Unsupported(String),
    /// No match where at least one was required.
    NotFound { message: String },
    /// 2+ matches where exactly one was required. Lists every match so the
    /// caller can disambiguate (`nth`/`first`/`within`/tighter query).
    Ambiguous { matches: Vec<Span> },
    /// A retryable assertion never reached its condition before its ONE
    /// deadline.
    Timeout { waited: Duration, reason: String },
    /// Action refused: the target lives in scrollback, which has no viewport
    /// coordinates to click.
    ViewportOnly { span: Span },
    /// Action refused: the screen revision changed after readiness was
    /// established. The click/submit was NOT delivered.
    StaleTarget { expected: u64, current: u64 },
    /// [`Locator::remains_absent`] observed a match during the watch window.
    UnexpectedlyPresent { matches: Vec<Span> },
}

impl LocateError {
    /// True for [`LocateError::Usage`] and [`LocateError::Unsupported`]:
    /// retry loops must return these immediately.
    #[must_use]
    pub fn is_immediate(&self) -> bool {
        matches!(self, Self::Usage(_) | Self::Unsupported(_))
    }

    fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound {
            message: msg.into(),
        }
    }
}

impl std::fmt::Display for LocateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(m) => write!(f, "locator usage error: {m}"),
            Self::Unsupported(m) => write!(f, "locator unsupported: {m}"),
            Self::NotFound { message } => write!(f, "locator found nothing: {message}"),
            Self::Ambiguous { matches } => {
                write!(f, "locator ambiguous: {} matches: ", matches.len())?;
                for (i, s) in matches.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{s}")?;
                }
                Ok(())
            }
            Self::Timeout { waited, reason } => {
                write!(f, "locator timed out after {waited:?}: {reason}")
            }
            Self::ViewportOnly { span } => {
                write!(f, "locator target is scrollback, not viewport: {span}")
            }
            Self::StaleTarget { expected, current } => write!(
                f,
                "locator target stale: prepared at revision {expected}, now {current}"
            ),
            Self::UnexpectedlyPresent { matches } => {
                write!(f, "locator matched {} time(s) while absent", matches.len())
            }
        }
    }
}

impl std::error::Error for LocateError {}

// ---------------------------------------------------------------------------
// Span
// ---------------------------------------------------------------------------

/// One match: terminal cell coordinates plus provenance.
///
/// - Viewport spans: `(x, y)` is the grid-local start cell, `(end_x, end_y)`
///   the exclusive end (single-row matches have `end_y == y`).
/// - Scrollback spans: `scrollback` is true, `scrollback_index` is the line
///   index (oldest = 0), `x`/`end_x` are char offsets in that line, and `y` is
///   always 0 (there are no viewport rows to click).
/// - `origin` is the resolved screen's origin (region crops keep theirs, Q10).
/// - `width_cols` is display columns covered (wide leads count 2; wrapped
///   matches span rows, see docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub x: u16,
    pub y: u16,
    pub end_x: u16,
    pub end_y: u16,
    pub width_cols: u16,
    pub origin: (i32, i32),
    pub revision: u64,
    pub text: String,
    pub scrollback: bool,
    pub scrollback_index: Option<usize>,
}

impl Span {
    /// Total order key: scrollback (oldest first) before viewport (row-major).
    fn key(&self) -> (u8, usize, u16) {
        if self.scrollback {
            (0, self.scrollback_index.unwrap_or(0), self.x)
        } else {
            (1, self.y as usize, self.x)
        }
    }

    fn end_key(&self) -> (u8, usize, u16) {
        if self.scrollback {
            (0, self.scrollback_index.unwrap_or(0), self.end_x)
        } else {
            (1, self.end_y as usize, self.end_x)
        }
    }

    /// Click target: viewport start cell. Scrollback has none.
    #[must_use]
    pub fn click_point(&self) -> Option<(u16, u16)> {
        if self.scrollback {
            None
        } else {
            Some((self.x, self.y))
        }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.scrollback {
            write!(
                f,
                "scrollback[{}] chars {}..{} {:?}",
                self.scrollback_index.unwrap_or(0),
                self.x,
                self.end_x,
                self.text
            )
        } else if self.end_y == self.y {
            write!(
                f,
                "({},{})..({},{}) {:?}",
                self.x, self.y, self.end_x, self.end_y, self.text
            )
        } else {
            write!(
                f,
                "({},{})..({},{}) wrapped {:?}",
                self.x, self.y, self.end_x, self.end_y, self.text
            )
        }
    }
}

/// Delivered action. Sinks receive exactly one of these per `click`/`submit`
/// call; readiness retries never touch the sink (Q05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Click { x: u16, y: u16 },
    Submit { x: u16, y: u16 },
}

// ---------------------------------------------------------------------------
// Text match modes + style queries
// ---------------------------------------------------------------------------

/// How [`Locator::text`] interprets its pattern.
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

/// Cell-style predicate for [`Locator::style`]. Every specified field must
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
    strikethrough: Option<bool>,
    reverse: Option<bool>,
    hidden: Option<bool>,
    blink: Option<bool>,
    custom: Option<fn(&Cell) -> bool>,
}

impl StyleQuery {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn fg(mut self, c: Color) -> Self {
        self.fg = Some(c);
        self
    }

    #[must_use]
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = Some(c);
        self
    }

    #[must_use]
    pub fn bold(mut self, v: bool) -> Self {
        self.bold = Some(v);
        self
    }

    #[must_use]
    pub fn dim(mut self, v: bool) -> Self {
        self.dim = Some(v);
        self
    }

    #[must_use]
    pub fn italic(mut self, v: bool) -> Self {
        self.italic = Some(v);
        self
    }

    #[must_use]
    pub fn underline(mut self, v: bool) -> Self {
        self.underline = Some(v);
        self
    }

    #[must_use]
    pub fn strikethrough(mut self, v: bool) -> Self {
        self.strikethrough = Some(v);
        self
    }

    #[must_use]
    pub fn reverse(mut self, v: bool) -> Self {
        self.reverse = Some(v);
        self
    }

    #[must_use]
    pub fn hidden(mut self, v: bool) -> Self {
        self.hidden = Some(v);
        self
    }

    #[must_use]
    pub fn blink(mut self, v: bool) -> Self {
        self.blink = Some(v);
        self
    }

    /// Extra caller predicate, ANDed with the field constraints.
    #[must_use]
    pub fn custom(mut self, p: fn(&Cell) -> bool) -> Self {
        self.custom = Some(p);
        self
    }

    fn matches(&self, cell: &Cell) -> bool {
        if cell.continuation {
            return false;
        }
        if let Some(fg) = self.fg {
            if cell.fg != fg {
                return false;
            }
        }
        if let Some(bg) = self.bg {
            if cell.bg != bg {
                return false;
            }
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
            if let Some(w) = want {
                if w != got {
                    return false;
                }
            }
        }
        if let Some(p) = self.custom {
            if !p(cell) {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Minimal regex subset (see module docs for limits)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReToken {
    Start,
    End,
    Dot,
    Lit(char),
    Class {
        ranges: Vec<(char, char)>,
        negated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReAtom {
    token: ReToken,
    min: usize,
    max: Option<usize>, // None = unbounded
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MiniRegex {
    atoms: Vec<ReAtom>,
    case_insensitive: bool,
}

fn fold_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

impl MiniRegex {
    fn parse(pattern: &str, case_insensitive: bool) -> Result<Self, LocateError> {
        let usage = |m: String| LocateError::Usage(format!("bad regex {pattern:?}: {m}"));
        if pattern.is_empty() {
            return Err(usage("pattern is empty".to_string()));
        }
        let raw: Vec<char> = pattern.chars().collect();
        let mut atoms: Vec<ReAtom> = Vec::new();
        let mut i = 0;
        let push = |atoms: &mut Vec<ReAtom>, token: ReToken| {
            atoms.push(ReAtom {
                token,
                min: 1,
                max: Some(1),
            });
        };
        while i < raw.len() {
            let c = raw[i];
            match c {
                '^' if i == 0 => push(&mut atoms, ReToken::Start),
                '$' if i == raw.len() - 1 => push(&mut atoms, ReToken::End),
                '.' => push(&mut atoms, ReToken::Dot),
                '*' | '+' | '?' => {
                    let prev = atoms
                        .last_mut()
                        .ok_or_else(|| usage("dangling quantifier".to_string()))?;
                    if matches!(prev.token, ReToken::Start | ReToken::End) {
                        return Err(usage("quantifier on anchor".to_string()));
                    }
                    match c {
                        '*' => {
                            prev.min = 0;
                            prev.max = None;
                        }
                        '+' => {
                            prev.min = 1;
                            prev.max = None;
                        }
                        _ => {
                            prev.min = 0;
                            prev.max = Some(1);
                        }
                    }
                }
                '(' | ')' | '|' | '{' | '}' => {
                    return Err(usage(format!(
                        "unsupported construct {c:?}: no groups, alternation, or {{m,n}}"
                    )));
                }
                '[' => {
                    let (token, next) = Self::parse_class(&raw, i)?;
                    push(&mut atoms, token);
                    i = next;
                    continue;
                }
                '\\' => {
                    i += 1;
                    if i >= raw.len() {
                        return Err(usage("trailing backslash".to_string()));
                    }
                    match raw[i] {
                        'd' => push(
                            &mut atoms,
                            ReToken::Class {
                                ranges: vec![('0', '9')],
                                negated: false,
                            },
                        ),
                        'w' => push(
                            &mut atoms,
                            ReToken::Class {
                                ranges: vec![('A', 'Z'), ('a', 'z'), ('0', '9'), ('_', '_')],
                                negated: false,
                            },
                        ),
                        's' => push(
                            &mut atoms,
                            ReToken::Class {
                                ranges: vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')],
                                negated: false,
                            },
                        ),
                        'D' => push(
                            &mut atoms,
                            ReToken::Class {
                                ranges: vec![('0', '9')],
                                negated: true,
                            },
                        ),
                        'W' => push(
                            &mut atoms,
                            ReToken::Class {
                                ranges: vec![('A', 'Z'), ('a', 'z'), ('0', '9'), ('_', '_')],
                                negated: true,
                            },
                        ),
                        'S' => push(
                            &mut atoms,
                            ReToken::Class {
                                ranges: vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')],
                                negated: true,
                            },
                        ),
                        lit => push(&mut atoms, ReToken::Lit(lit)),
                    }
                }
                lit => push(&mut atoms, ReToken::Lit(lit)),
            }
            i += 1;
        }
        let mut re = Self {
            atoms,
            case_insensitive,
        };
        if case_insensitive {
            re.fold_pattern();
        }
        Ok(re)
    }

    fn parse_class(raw: &[char], open: usize) -> Result<(ReToken, usize), LocateError> {
        let usage = |m: String| LocateError::Usage(format!("bad regex class: {m}"));
        let mut i = open + 1;
        let mut negated = false;
        if i < raw.len() && raw[i] == '^' {
            negated = true;
            i += 1;
        }
        let mut ranges: Vec<(char, char)> = Vec::new();
        let mut first = true;
        while i < raw.len() {
            let c = raw[i];
            if c == ']' && !first {
                if ranges.is_empty() {
                    return Err(usage("empty class".to_string()));
                }
                return Ok((ReToken::Class { ranges, negated }, i + 1));
            }
            first = false;
            let lo = if c == '\\' {
                i += 1;
                if i >= raw.len() {
                    return Err(usage("trailing backslash in class".to_string()));
                }
                match raw[i] {
                    'd' => {
                        ranges.push(('0', '9'));
                        i += 1;
                        continue;
                    }
                    'w' => {
                        ranges.extend([('A', 'Z'), ('a', 'z'), ('0', '9'), ('_', '_')]);
                        i += 1;
                        continue;
                    }
                    's' => {
                        ranges.extend([(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')]);
                        i += 1;
                        continue;
                    }
                    lit => lit,
                }
            } else {
                c
            };
            if i + 2 < raw.len() && raw[i + 1] == '-' && raw[i + 2] != ']' {
                let hi = raw[i + 2];
                if hi < lo {
                    return Err(usage(format!("reversed range {lo:?}-{hi:?}")));
                }
                ranges.push((lo, hi));
                i += 3;
            } else {
                ranges.push((lo, lo));
                i += 1;
            }
        }
        Err(usage("unclosed `[`".to_string()))
    }

    fn fold_pattern(&mut self) {
        for atom in &mut self.atoms {
            match &mut atom.token {
                ReToken::Lit(c) => *c = fold_char(*c),
                ReToken::Class { ranges, .. } => {
                    for (lo, hi) in ranges.iter_mut() {
                        *lo = fold_char(*lo);
                        *hi = fold_char(*hi);
                    }
                }
                _ => {}
            }
        }
    }

    fn atom_matches(token: &ReToken, c: char) -> bool {
        match token {
            ReToken::Dot => true,
            ReToken::Lit(l) => *l == c,
            ReToken::Class { ranges, negated } => {
                let hit = ranges.iter().any(|(lo, hi)| *lo <= c && c <= *hi);
                hit != *negated
            }
            ReToken::Start | ReToken::End => false,
        }
    }

    /// Greedy backtracking match of `atoms[ai..]` at `ci`; returns end index.
    fn match_rest(&self, ai: usize, chars: &[char], ci: usize) -> Option<usize> {
        if ai == self.atoms.len() {
            return Some(ci);
        }
        let atom = &self.atoms[ai];
        match &atom.token {
            ReToken::Start if ci == 0 => self.match_rest(ai + 1, chars, ci),
            ReToken::End if ci == chars.len() => self.match_rest(ai + 1, chars, ci),
            ReToken::Start | ReToken::End => None,
            _ => {
                let mut ends = vec![ci];
                let mut j = ci;
                while atom.max.is_none_or(|m| ends.len() - 1 < m)
                    && j < chars.len()
                    && Self::atom_matches(&atom.token, chars[j])
                {
                    j += 1;
                    ends.push(j);
                }
                for k in (atom.min..ends.len()).rev() {
                    if let Some(end) = self.match_rest(ai + 1, chars, ends[k]) {
                        return Some(end);
                    }
                }
                None
            }
        }
    }

    /// Leftmost non-overlapping matches as char ranges. Empty matches are
    /// skipped (documented): patterns that match empty advance one char.
    fn find_all(&self, chars: &[char]) -> Vec<(usize, usize)> {
        let hay: Vec<char> = if self.case_insensitive {
            chars.iter().map(|c| fold_char(*c)).collect()
        } else {
            chars.to_vec()
        };
        let mut out = Vec::new();
        let mut s = 0;
        while s <= hay.len() {
            match self.match_rest(0, &hay, s) {
                Some(e) if e > s => {
                    out.push((s, e));
                    s = e;
                }
                _ => s += 1,
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Locator
// ---------------------------------------------------------------------------

/// Query over a [`Screen`] at a revision. Built with `text`/`regex`/`style`/
/// `region`, refined with match modes, combined with `within`/`before`/`after`
/// /`nth`/`first`/`last`/`and`/`or`/`filter`.
///
/// Resolution order is total and documented: scrollback matches (oldest line
/// first) come before viewport matches (row-major), so `first`/`nth`/actions
/// are deterministic.
#[derive(Debug, Clone)]
pub struct Locator {
    kind: LocatorKind,
    /// Text/regex: join wrapped rows into logical lines (default true).
    join_wrapped: bool,
}

#[derive(Debug, Clone)]
enum LocatorKind {
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

    /// Resolve against a screen at a revision (viewport only).
    pub fn resolve(&self, screen: &Screen, revision: u64) -> Result<Vec<Span>, LocateError> {
        self.resolve_with_scrollback(screen, revision, &[])
    }

    /// Resolve against an [`Observation`] (viewport only).
    pub fn resolve_obs(&self, obs: &Observation) -> Result<Vec<Span>, LocateError> {
        self.resolve(&obs.screen, obs.revision)
    }

    /// Resolve against a screen plus scrollback lines (plain text, oldest
    /// first). Scrollback matches are flagged `scrollback: true` and are
    /// never clickable.
    pub fn resolve_with_scrollback(
        &self,
        screen: &Screen,
        revision: u64,
        scrollback: &[String],
    ) -> Result<Vec<Span>, LocateError> {
        let mut spans = self.resolve_core(screen, revision, scrollback)?;
        spans.sort_by(|a, b| a.key().cmp(&b.key()).then(a.end_key().cmp(&b.end_key())));
        Ok(spans)
    }

    /// Resolve requiring exactly one match: 0 → [`LocateError::NotFound`],
    /// 2+ → [`LocateError::Ambiguous`] listing every match (Q03 strictness).
    pub fn resolve_unique(&self, screen: &Screen, revision: u64) -> Result<Span, LocateError> {
        self.resolve_unique_with_scrollback(screen, revision, &[])
    }

    /// [`Locator::resolve_unique`] with scrollback lines included.
    pub fn resolve_unique_with_scrollback(
        &self,
        screen: &Screen,
        revision: u64,
        scrollback: &[String],
    ) -> Result<Span, LocateError> {
        let spans = self.resolve_with_scrollback(screen, revision, scrollback)?;
        match spans.len() {
            1 => Ok(spans.into_iter().next().expect("len checked")),
            0 => Err(LocateError::not_found("no matches")),
            _ => Err(LocateError::Ambiguous { matches: spans }),
        }
    }

    fn resolve_core(
        &self,
        screen: &Screen,
        revision: u64,
        scrollback: &[String],
    ) -> Result<Vec<Span>, LocateError> {
        match &self.kind {
            LocatorKind::Text { pattern, mode } => {
                Self::check_text_pattern(pattern)?;
                let mut out = Vec::new();
                for (idx, line) in scrollback.iter().enumerate() {
                    out.extend(match_text_scrollback(
                        pattern,
                        *mode,
                        line,
                        idx,
                        screen.origin(),
                        revision,
                    )?);
                }
                out.extend(match_text_viewport(
                    pattern,
                    *mode,
                    self.join_wrapped,
                    screen,
                    revision,
                )?);
                Ok(out)
            }
            LocatorKind::Regex { source, re } => {
                let mut out = Vec::new();
                for (idx, line) in scrollback.iter().enumerate() {
                    let chars: Vec<char> = line.chars().collect();
                    Self::check_regex_haystack(re, &chars, source)?;
                    out.extend(match_regex_scrollback(
                        re,
                        &chars,
                        idx,
                        screen.origin(),
                        revision,
                    ));
                }
                out.extend(match_regex_viewport(
                    re,
                    source,
                    self.join_wrapped,
                    screen,
                    revision,
                )?);
                Ok(out)
            }
            // Scrollback carries no cell geometry, so style/region locators
            // are viewport-only by construction (documented, not an error).
            LocatorKind::Style { query } => Ok(match_style_viewport(query, screen, revision)),
            LocatorKind::Region { x, y, cols, rows } => {
                match_region_viewport(*x, *y, *cols, *rows, screen, revision)
            }
            LocatorKind::Within { scope, inner } => {
                let outer = scope.resolve_core(screen, revision, scrollback)?;
                let inner = inner.resolve_core(screen, revision, scrollback)?;
                Ok(inner
                    .into_iter()
                    .filter(|s| outer.iter().any(|o| contains(o, s)))
                    .collect())
            }
            LocatorKind::Before { main, anchor } => {
                let main = main.resolve_core(screen, revision, scrollback)?;
                let anchor = anchor.resolve_core(screen, revision, scrollback)?;
                Ok(main
                    .into_iter()
                    .filter(|s| anchor.iter().any(|a| a.key() > s.key()))
                    .collect())
            }
            LocatorKind::After { main, anchor } => {
                let main = main.resolve_core(screen, revision, scrollback)?;
                let anchor = anchor.resolve_core(screen, revision, scrollback)?;
                Ok(main
                    .into_iter()
                    .filter(|s| anchor.iter().any(|a| a.key() < s.key()))
                    .collect())
            }
            LocatorKind::Nth { inner, index } => {
                let mut spans = inner.resolve_core(screen, revision, scrollback)?;
                spans.sort_by_key(|a| a.key());
                Ok(spans.into_iter().nth(*index).into_iter().collect())
            }
            LocatorKind::First(inner) => {
                let mut spans = inner.resolve_core(screen, revision, scrollback)?;
                spans.sort_by_key(|a| a.key());
                Ok(spans.into_iter().next().into_iter().collect())
            }
            LocatorKind::Last(inner) => {
                let mut spans = inner.resolve_core(screen, revision, scrollback)?;
                spans.sort_by_key(|a| a.key());
                Ok(spans.into_iter().last().into_iter().collect())
            }
            LocatorKind::And { a, b } => {
                let a = a.resolve_core(screen, revision, scrollback)?;
                let b = b.resolve_core(screen, revision, scrollback)?;
                Ok(a.into_iter()
                    .filter(|s| b.iter().any(|o| overlaps(s, o)))
                    .collect())
            }
            LocatorKind::Or { a, b } => {
                let mut out = a.resolve_core(screen, revision, scrollback)?;
                for s in b.resolve_core(screen, revision, scrollback)? {
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
                Ok(out)
            }
            LocatorKind::Filter { inner, pred } => {
                let spans = inner.resolve_core(screen, revision, scrollback)?;
                Ok(spans.into_iter().filter(pred).collect())
            }
        }
    }

    fn check_text_pattern(pattern: &str) -> Result<(), LocateError> {
        if pattern.is_empty() {
            return Err(LocateError::Usage("text pattern is empty".to_string()));
        }
        if pattern.contains('\n') {
            return Err(LocateError::Usage(
                "text pattern contains newline; match one logical line (wrapped-line joining is on by default)".to_string(),
            ));
        }
        Ok(())
    }

    fn check_regex_haystack(
        re: &MiniRegex,
        chars: &[char],
        source: &str,
    ) -> Result<(), LocateError> {
        if re.case_insensitive && chars.iter().any(|c| !c.is_ascii()) {
            return Err(LocateError::Unsupported(format!(
                "case-insensitive regex {source:?} over non-ASCII text: first-char lowering is unreliable"
            )));
        }
        Ok(())
    }
}

/// True when `outer` fully contains `inner` (same area, range inside).
fn contains(outer: &Span, inner: &Span) -> bool {
    if outer.scrollback != inner.scrollback {
        return false;
    }
    if outer.scrollback {
        return outer.scrollback_index == inner.scrollback_index
            && outer.x <= inner.x
            && inner.end_x <= outer.end_x;
    }
    outer.key() <= inner.key() && inner.end_key() <= outer.end_key()
}

/// True when both spans share an area+line and their column ranges intersect.
fn overlaps(a: &Span, b: &Span) -> bool {
    if a.scrollback != b.scrollback {
        return false;
    }
    if a.scrollback {
        if a.scrollback_index != b.scrollback_index {
            return false;
        }
    } else if a.y != b.y || a.end_y != b.end_y {
        return false;
    }
    a.x < b.end_x && b.x < a.end_x
}

// ---------------------------------------------------------------------------
// Row text extraction (wide-cell aware)
// ---------------------------------------------------------------------------

/// One physical row as chars with per-char grid columns. Continuation cells
/// contribute no chars; each char of a lead symbol maps to the lead's column,
/// and the row's end column accounts for the last lead's display width.
/// Trailing blank cells are excluded.
struct RowText {
    chars: Vec<char>,
    /// Grid column of each char's lead cell.
    cols: Vec<u16>,
    /// Display width of each char's lead cell.
    widths: Vec<u8>,
}

impl RowText {
    fn extract(screen: &Screen, y: u16) -> Self {
        let mut chars = Vec::new();
        let mut cols = Vec::new();
        let mut widths = Vec::new();
        for x in 0..screen.cols() {
            let cell = screen.get(x, y).expect("row in range");
            if cell.continuation {
                continue;
            }
            for c in cell.symbol.chars() {
                chars.push(c);
                cols.push(cell.x);
                widths.push(cell.width);
            }
        }
        while chars.last() == Some(&' ') {
            chars.pop();
            cols.pop();
            widths.pop();
        }
        Self {
            chars,
            cols,
            widths,
        }
    }

    /// Wrap heuristic: the row's last column holds a non-blank cell, so text
    /// runs to the edge and (by convention) continues on the next row.
    fn row_full(screen: &Screen, y: u16) -> bool {
        let last = screen.cols() - 1;
        let cell = screen.get(last, y).expect("row in range");
        if cell.continuation {
            return true;
        }
        cell.symbol != " " && !cell.symbol.is_empty()
    }
}

/// A logical line: one physical row, or consecutive rows joined by the wrap
/// heuristic. `row_of[i]`/`col_of[i]`/`width_of[i]` locate char `i`.
struct LogicalLine {
    chars: Vec<char>,
    rows: Vec<u16>,
    cols: Vec<u16>,
    widths: Vec<u8>,
    end_row: u16,
}

fn logical_lines(screen: &Screen, join_wrapped: bool) -> Vec<LogicalLine> {
    let mut lines = Vec::new();
    let mut y = 0;
    while y < screen.rows() {
        let mut line = LogicalLine {
            chars: Vec::new(),
            rows: Vec::new(),
            cols: Vec::new(),
            widths: Vec::new(),
            end_row: y,
        };
        loop {
            let row = RowText::extract(screen, y);
            for i in 0..row.chars.len() {
                line.chars.push(row.chars[i]);
                line.rows.push(y);
                line.cols.push(row.cols[i]);
                line.widths.push(row.widths[i]);
            }
            line.end_row = y;
            if !join_wrapped || y + 1 >= screen.rows() || !RowText::row_full(screen, y) {
                break;
            }
            y += 1;
        }
        lines.push(line);
        y += 1;
    }
    lines
}

fn span_for_char_range(
    line: &LogicalLine,
    start: usize,
    end: usize,
    origin: (i32, i32),
    revision: u64,
    cols: u16,
) -> Span {
    let text: String = line.chars[start..end].iter().collect();
    let x = line.cols[start];
    let y = line.rows[start];
    let (end_x, end_y) = if end > start {
        let last = end - 1;
        (
            line.cols[last] + u16::from(line.widths[last]),
            line.rows[last],
        )
    } else {
        (x, y)
    };
    let width_cols = if end_y == y {
        end_x.saturating_sub(x)
    } else {
        (end_y - y - 1) * cols + (cols - x) + end_x
    };
    Span {
        x,
        y,
        end_x,
        end_y,
        width_cols,
        origin,
        revision,
        text,
        scrollback: false,
        scrollback_index: None,
    }
}

// ---------------------------------------------------------------------------
// Text matching
// ---------------------------------------------------------------------------

/// Collapse every whitespace run to one space; returns normalized chars plus
/// the source index of each normalized char.
fn normalize_with_map(chars: &[char]) -> (Vec<char>, Vec<usize>) {
    let mut out = Vec::new();
    let mut map = Vec::new();
    let mut in_ws = true; // also trims leading whitespace
    for (i, c) in chars.iter().enumerate() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
                map.push(i);
                in_ws = true;
            }
        } else {
            out.push(*c);
            map.push(i);
            in_ws = false;
        }
    }
    if out.last() == Some(&' ') {
        out.pop();
        map.pop();
    }
    (out, map)
}

fn find_substring(hay: &[char], needle: &[char]) -> Vec<(usize, usize)> {
    if needle.is_empty() || needle.len() > hay.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut s = 0;
    while s + needle.len() <= hay.len() {
        if hay[s..s + needle.len()] == needle[..] {
            out.push((s, s + needle.len()));
            s += needle.len();
        } else {
            s += 1;
        }
    }
    out
}

/// Char ranges of `pattern` in `chars` under `mode` (scrollback/logical-line
/// shared core; match text is the ORIGINAL slice).
fn text_ranges(pattern: &str, mode: TextMode, chars: &[char]) -> Vec<(usize, usize)> {
    match mode {
        TextMode::Substring => {
            let needle: Vec<char> = pattern.chars().collect();
            find_substring(chars, &needle)
        }
        TextMode::Exact => {
            let text: String = chars.iter().collect();
            if text == pattern {
                vec![(0, chars.len())]
            } else {
                Vec::new()
            }
        }
        TextMode::CaseInsensitive => {
            let needle: Vec<char> = pattern.chars().map(fold_char).collect();
            let folded: Vec<char> = chars.iter().map(|c| fold_char(*c)).collect();
            find_substring(&folded, &needle)
        }
        TextMode::Normalized => {
            let (hay_n, map) = normalize_with_map(chars);
            let pat_chars: Vec<char> = pattern.chars().collect();
            let (pat_n, _) = normalize_with_map(&pat_chars);
            find_substring(&hay_n, &pat_n)
                .into_iter()
                .map(|(s, e)| (map[s], map[e - 1] + 1))
                .collect()
        }
    }
}

fn match_text_scrollback(
    pattern: &str,
    mode: TextMode,
    line: &str,
    index: usize,
    origin: (i32, i32),
    revision: u64,
) -> Result<Vec<Span>, LocateError> {
    let chars: Vec<char> = line.chars().collect();
    if mode == TextMode::CaseInsensitive && chars.iter().any(|c| !c.is_ascii()) {
        return Err(LocateError::Unsupported(
            "case-insensitive text over non-ASCII text: first-char lowering is unreliable"
                .to_string(),
        ));
    }
    Ok(text_ranges(pattern, mode, &chars)
        .into_iter()
        .map(|(s, e)| {
            let text: String = chars[s..e].iter().collect();
            Span {
                x: s as u16,
                y: 0,
                end_x: e as u16,
                end_y: 0,
                width_cols: (e - s) as u16,
                origin,
                revision,
                text,
                scrollback: true,
                scrollback_index: Some(index),
            }
        })
        .collect())
}

fn match_text_viewport(
    pattern: &str,
    mode: TextMode,
    join_wrapped: bool,
    screen: &Screen,
    revision: u64,
) -> Result<Vec<Span>, LocateError> {
    let mut out = Vec::new();
    for line in logical_lines(screen, join_wrapped) {
        if mode == TextMode::CaseInsensitive && line.chars.iter().any(|c| !c.is_ascii()) {
            return Err(LocateError::Unsupported(
                "case-insensitive text over non-ASCII text: first-char lowering is unreliable"
                    .to_string(),
            ));
        }
        for (s, e) in text_ranges(pattern, mode, &line.chars) {
            out.push(span_for_char_range(
                &line,
                s,
                e,
                screen.origin(),
                revision,
                screen.cols(),
            ));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Regex matching
// ---------------------------------------------------------------------------

fn match_regex_scrollback(
    re: &MiniRegex,
    chars: &[char],
    index: usize,
    origin: (i32, i32),
    revision: u64,
) -> Vec<Span> {
    re.find_all(chars)
        .into_iter()
        .map(|(s, e)| {
            let text: String = chars[s..e].iter().collect();
            Span {
                x: s as u16,
                y: 0,
                end_x: e as u16,
                end_y: 0,
                width_cols: (e - s) as u16,
                origin,
                revision,
                text,
                scrollback: true,
                scrollback_index: Some(index),
            }
        })
        .collect()
}

fn match_regex_viewport(
    re: &MiniRegex,
    source: &str,
    join_wrapped: bool,
    screen: &Screen,
    revision: u64,
) -> Result<Vec<Span>, LocateError> {
    let mut out = Vec::new();
    for line in logical_lines(screen, join_wrapped) {
        Locator::check_regex_haystack(re, &line.chars, source)?;
        for (s, e) in re.find_all(&line.chars) {
            out.push(span_for_char_range(
                &line,
                s,
                e,
                screen.origin(),
                revision,
                screen.cols(),
            ));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Style + region matching (viewport only)
// ---------------------------------------------------------------------------

fn match_style_viewport(query: &StyleQuery, screen: &Screen, revision: u64) -> Vec<Span> {
    let mut out = Vec::new();
    for y in 0..screen.rows() {
        let mut run_start: Option<u16> = None;
        let mut run_text = String::new();
        let mut run_end = 0u16;
        let flush = |out: &mut Vec<Span>,
                     run_start: &mut Option<u16>,
                     run_text: &mut String,
                     run_end: u16| {
            if let Some(x) = run_start.take() {
                out.push(Span {
                    x,
                    y,
                    end_x: run_end,
                    end_y: y,
                    width_cols: run_end.saturating_sub(x),
                    origin: screen.origin(),
                    revision,
                    text: std::mem::take(run_text),
                    scrollback: false,
                    scrollback_index: None,
                });
            }
        };
        for x in 0..screen.cols() {
            let cell = screen.get(x, y).expect("cell in range");
            if cell.continuation {
                continue;
            }
            if query.matches(cell) {
                if run_start.is_none() {
                    run_start = Some(x);
                }
                run_text.push_str(&cell.symbol);
                run_end = x + u16::from(cell.width);
            } else {
                flush(&mut out, &mut run_start, &mut run_text, run_end);
            }
        }
        flush(&mut out, &mut run_start, &mut run_text, run_end);
    }
    out
}

fn match_region_viewport(
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
    screen: &Screen,
    revision: u64,
) -> Result<Vec<Span>, LocateError> {
    if cols == 0 || rows == 0 {
        return Err(LocateError::Usage(
            "region dimensions must be nonzero".to_string(),
        ));
    }
    if x as u32 + cols as u32 > screen.cols() as u32
        || y as u32 + rows as u32 > screen.rows() as u32
    {
        return Err(LocateError::Usage(format!(
            "region ({x},{y}) {cols}x{rows} outside {}x{} screen",
            screen.cols(),
            screen.rows()
        )));
    }
    // Never silently cut a wide grapheme (Q10): same rule as Screen::region.
    for r in y..y + rows {
        let left = screen.get(x, r).expect("row checked");
        if left.continuation {
            return Err(LocateError::Usage(format!(
                "region left edge ({x},{r}) splits a wide grapheme"
            )));
        }
        let right = screen.get(x + cols - 1, r).expect("row checked");
        if right.width == 2 && !right.continuation {
            return Err(LocateError::Usage(format!(
                "region right edge ({},{r}) splits a wide grapheme",
                x + cols - 1
            )));
        }
    }
    let mut out = Vec::new();
    for r in y..y + rows {
        let mut text = String::new();
        for c in x..x + cols {
            let cell = screen.get(c, r).expect("cell checked");
            if !cell.continuation {
                text.push_str(&cell.symbol);
            }
        }
        out.push(Span {
            x,
            y: r,
            end_x: x + cols,
            end_y: r,
            width_cols: cols,
            origin: screen.origin(),
            revision,
            text,
            scrollback: false,
            scrollback_index: None,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Retryable assertions (ONE deadline each) + temporal assertions
// ---------------------------------------------------------------------------

/// Fixed poll interval for all retry loops.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

impl Locator {
    /// Retry until at least one match, or the ONE `timeout` deadline.
    /// [`LocateError::Usage`]/[`LocateError::Unsupported`] fail immediately.
    pub fn expect_visible<F>(
        &self,
        observe: &mut F,
        timeout: Duration,
    ) -> Result<Vec<Span>, LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            match self.resolve_obs(&obs) {
                Ok(spans) if !spans.is_empty() => return Ok(spans),
                Ok(_) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: "no match became visible".to_string(),
                        });
                    }
                }
                Err(e) if e.is_immediate() => return Err(e),
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("still failing: {e}"),
                        });
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Retry until exactly one match whose text equals `expected`, or the ONE
    /// `timeout` deadline. Wrong text / zero / 2+ matches all keep retrying
    /// (the screen may still be settling); usage/unsupported fail immediately.
    pub fn expect_text<F>(
        &self,
        observe: &mut F,
        expected: &str,
        timeout: Duration,
    ) -> Result<Span, LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            let state: Result<Option<Span>, LocateError> = match self.resolve_obs(&obs) {
                Ok(spans) if spans.len() == 1 && spans[0].text == expected => {
                    return Ok(spans.into_iter().next().expect("len checked"));
                }
                Ok(spans) if spans.len() == 1 => Ok(None),
                Ok(_) => Ok(None),
                Err(e) if e.is_immediate() => return Err(e),
                Err(e) => Err(e),
            };
            if Instant::now() >= deadline {
                let reason = match state {
                    Ok(_) => format!("no unique match with text {expected:?}"),
                    Err(e) => format!("still failing: {e}"),
                };
                return Err(LocateError::Timeout {
                    waited: timeout,
                    reason,
                });
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Retry until the match count equals `expected`, or the ONE `timeout`
    /// deadline. Usage/unsupported fail immediately.
    pub fn expect_count<F>(
        &self,
        observe: &mut F,
        expected: usize,
        timeout: Duration,
    ) -> Result<Vec<Span>, LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            match self.resolve_obs(&obs) {
                Ok(spans) if spans.len() == expected => return Ok(spans),
                Ok(spans) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("count {} != expected {expected}", spans.len()),
                        });
                    }
                }
                Err(e) if e.is_immediate() => return Err(e),
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("still failing: {e}"),
                        });
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Single-shot presence check against one observation (no retry).
    pub fn present_now(&self, obs: &Observation) -> Result<bool, LocateError> {
        Ok(!self.resolve_obs(obs)?.is_empty())
    }

    /// Single-shot absence check against one observation (no retry).
    pub fn not_present_now(&self, obs: &Observation) -> Result<bool, LocateError> {
        Ok(self.resolve_obs(obs)?.is_empty())
    }

    /// Retry until zero matches, or the ONE `timeout` deadline.
    /// Usage/unsupported fail immediately.
    pub fn eventually_absent<F>(
        &self,
        observe: &mut F,
        timeout: Duration,
    ) -> Result<(), LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            match self.resolve_obs(&obs) {
                Ok(spans) if spans.is_empty() => return Ok(()),
                Ok(spans) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("still {} match(es) present", spans.len()),
                        });
                    }
                }
                Err(e) if e.is_immediate() => return Err(e),
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("still failing: {e}"),
                        });
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Watch for the FULL `duration`: any match at any poll fails with
    /// [`LocateError::UnexpectedlyPresent`]. Usage/unsupported fail
    /// immediately. Returns `Ok(())` only after the whole window stays empty.
    pub fn remains_absent<F>(&self, observe: &mut F, duration: Duration) -> Result<(), LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + duration;
        loop {
            let obs = observe();
            match self.resolve_obs(&obs) {
                Ok(spans) if spans.is_empty() => {}
                Ok(spans) => {
                    return Err(LocateError::UnexpectedlyPresent { matches: spans });
                }
                Err(e) if e.is_immediate() => return Err(e),
                Err(_) => {}
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

// ---------------------------------------------------------------------------
// Actions: prepare once (retryable readiness), deliver once (stale-refusing)
// ---------------------------------------------------------------------------

/// A readiness-established action target. Built by resolving a locator to a
/// UNIQUE viewport span at one revision; delivered by [`PendingAction::click`]
/// /[`PendingAction::submit`], which re-validate the unique target against the
/// CURRENT observation and refuse stale revisions. The sink runs at most once
/// per `click`/`submit` call, and readiness retries never touch the sink.
#[derive(Debug, Clone)]
pub struct PendingAction {
    locator: Locator,
    span: Span,
    revision: u64,
}

impl Locator {
    /// Single-shot readiness: unique viewport target at this observation's
    /// revision. Scrollback-only matches fail with
    /// [`LocateError::ViewportOnly`].
    pub fn prepare_action(&self, obs: &Observation) -> Result<PendingAction, LocateError> {
        let span = self.resolve_unique(&obs.screen, obs.revision)?;
        PendingAction::from_span(self.clone(), span, obs.revision)
    }

    /// Retryable readiness: poll until a unique viewport target exists, or
    /// the ONE `timeout` deadline. Never invokes any action sink (Q05).
    pub fn prepare_action_retry<F>(
        &self,
        observe: &mut F,
        timeout: Duration,
    ) -> Result<PendingAction, LocateError>
    where
        F: FnMut() -> Observation,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let obs = observe();
            match self.prepare_action(&obs) {
                Ok(pending) => return Ok(pending),
                Err(e) if e.is_immediate() => return Err(e),
                Err(LocateError::ViewportOnly { .. }) => {
                    // A scrollback-only target never becomes clickable by
                    // waiting on viewport revisions; still, the anchor text
                    // may scroll into view, so keep retrying to the deadline.
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: "target stayed outside the viewport".to_string(),
                        });
                    }
                }
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(LocateError::Timeout {
                            waited: timeout,
                            reason: format!("no unique target: {e}"),
                        });
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

impl PendingAction {
    /// Bind an already-resolved span. Fails with [`LocateError::ViewportOnly`]
    /// for scrollback spans and [`LocateError::Usage`] when the span's
    /// revision differs from `revision` (a cross-revision bind is meaningless).
    pub fn from_span(locator: Locator, span: Span, revision: u64) -> Result<Self, LocateError> {
        if span.scrollback {
            return Err(LocateError::ViewportOnly { span });
        }
        if span.revision != revision {
            return Err(LocateError::Usage(format!(
                "span revision {} != bind revision {revision}",
                span.revision
            )));
        }
        Ok(Self {
            locator,
            span,
            revision,
        })
    }

    #[must_use]
    pub fn span(&self) -> &Span {
        &self.span
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Re-validate the unique target against `current`, then deliver
    /// [`Action::Click`] to `sink` exactly once. Revision mismatch fails with
    /// [`LocateError::StaleTarget`] WITHOUT delivering or resolving further:
    /// a click is never sent stale.
    pub fn click(
        &self,
        current: &Observation,
        sink: &mut dyn FnMut(Action),
    ) -> Result<(), LocateError> {
        self.deliver(current, sink, false)
    }

    /// Like [`PendingAction::click`] but delivers [`Action::Submit`].
    pub fn submit(
        &self,
        current: &Observation,
        sink: &mut dyn FnMut(Action),
    ) -> Result<(), LocateError> {
        self.deliver(current, sink, true)
    }

    fn deliver(
        &self,
        current: &Observation,
        sink: &mut dyn FnMut(Action),
        submit: bool,
    ) -> Result<(), LocateError> {
        if current.revision != self.revision {
            return Err(LocateError::StaleTarget {
                expected: self.revision,
                current: current.revision,
            });
        }
        // Same revision: re-resolve to prove the target is still unique
        // pre-delivery (Q03 strictness at the delivery boundary).
        let fresh = self
            .locator
            .resolve_unique(&current.screen, current.revision)?;
        debug_assert_eq!(fresh, self.span, "same revision must resolve identically");
        let (x, y) = self.span.click_point().expect("viewport span");
        sink(if submit {
            Action::Submit { x, y }
        } else {
            Action::Click { x, y }
        });
        Ok(())
    }
}
