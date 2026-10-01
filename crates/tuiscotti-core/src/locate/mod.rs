//! M4 locator core (backlog Q01-Q05, Q08, Q10): Playwright-style queries over
//! [`Screen`](crate::screen::Screen) plus a revision (`u64`, usually from
//! [`Observation`](crate::screen::Observation)).
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
//! [`Screen`](crate::screen::Screen) carries viewport cells only, so scrollback lines (plain text,
//! oldest first) are supplied per call to
//! [`Locator::resolve_with_scrollback`]. Scrollback spans carry
//! `scrollback_index` and `scrollback: true`. Style/region locators ignore
//! scrollback (it has no cell geometry); text/regex combinators apply to both.

mod action;
mod error;
mod locator;
mod query;
mod regex;
mod resolve;
mod retry;
mod rows;
mod span;
mod text;
mod viewport;

pub use action::PendingAction;
pub use error::LocateError;
pub use locator::Locator;
pub use query::{StyleQuery, TextMode};
pub use span::{Action, Span};
