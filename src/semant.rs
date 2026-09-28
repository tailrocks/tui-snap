//! Semantic provider + deterministic event harness (backlog Q06, Q07, Q09).
//!
//! - [`SemanticProvider`]: explicit role/id/label/focused/disabled/hit-region
//!   data. Semantics NEVER come from appearance: nothing here inspects pixels,
//!   glyphs, styles, or [`Screen`](crate::screen::Screen) cells.
//! - Locators ([`by_role`], [`by_id`], [`by_label`]) resolve provider nodes to
//!   hit-region-center screen coordinates for REAL input. They return coords
//!   only and never call application controllers (Q07).
//! - [`RatatuiTestAdapter`]: example provider fed by test code alongside a
//!   draw closure; tests map widget areas to nodes manually.
//! - [`Harness`]: deterministic `update`/`render` + manual clock harness for
//!   runtime tests (Q09). No live clock, threads, or services.

use crate::ratatui::{render_screen, EdgePolicy};
use crate::screen::Screen;

/// Widget role. Fixed set; providers needing more map them onto these or use
/// [`Role::Static`] for non-interactive text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Role {
    Button,
    Textbox,
    Checkbox,
    ListItem,
    Link,
    Static,
}

/// Hit region in screen grid coordinates (same space as [`Screen`] cells).
/// The locator clicks its center.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HitRegion {
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
}

impl HitRegion {
    #[must_use]
    pub fn center(&self) -> (u16, u16) {
        (self.x + self.cols / 2, self.y + self.rows / 2)
    }
}

/// One semantic node: explicit provider data only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemNode {
    pub role: Role,
    pub id: Option<String>,
    pub label: Option<String>,
    pub focused: bool,
    pub disabled: bool,
    pub hit: HitRegion,
}

impl SemNode {
    #[must_use]
    pub fn new(role: Role, hit: HitRegion) -> Self {
        Self {
            role,
            id: None,
            label: None,
            focused: false,
            disabled: false,
            hit,
        }
    }

    #[must_use]
    pub fn with_id(mut self, id: &str) -> Self {
        self.id = Some(id.to_string());
        self
    }

    #[must_use]
    pub fn with_label(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    #[must_use]
    pub fn focused(mut self) -> Self {
        self.focused = true;
        self
    }

    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// Explicit semantic data source (Q06).
///
/// The provider revision tracks the screen revision the nodes were built for.
/// Locators refuse to resolve when the two differ ([`SemanticError::Stale`]).
pub trait SemanticProvider {
    fn revision(&self) -> u64;
    fn nodes(&self) -> &[SemNode];
}

/// Locator resolution failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticError {
    /// No enabled node matched.
    NotFound(String),
    /// More than one enabled node matched; disambiguate.
    Ambiguous { what: String, count: usize },
    /// The matched node is disabled: excluded from click targets.
    Disabled(String),
    /// Provider data is for another screen revision; re-capture first.
    Stale {
        screen_revision: u64,
        provider_revision: u64,
    },
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SemanticError::NotFound(w) => write!(f, "no semantic node matches {w}"),
            SemanticError::Ambiguous { what, count } => {
                write!(f, "{count} semantic nodes match {what}: disambiguate")
            }
            SemanticError::Disabled(w) => {
                write!(f, "semantic node {w} is disabled: not a click target")
            }
            SemanticError::Stale {
                screen_revision,
                provider_revision,
            } => write!(
                f,
                "stale semantics: screen revision {screen_revision} != provider revision {provider_revision}"
            ),
        }
    }
}

impl std::error::Error for SemanticError {}

/// Check provider freshness against the screen revision under test.
fn check_fresh(
    provider: &impl SemanticProvider,
    screen_revision: u64,
) -> Result<(), SemanticError> {
    let pr = provider.revision();
    if pr != screen_revision {
        return Err(SemanticError::Stale {
            screen_revision,
            provider_revision: pr,
        });
    }
    Ok(())
}

/// Resolve matches to one clickable coordinate.
///
/// Disabled nodes are excluded from click targets: if every match is disabled
/// the resolution fails with [`SemanticError::Disabled`]; disabled matches
/// never win over enabled ones.
fn pick(what: String, matches: Vec<&SemNode>) -> Result<(u16, u16), SemanticError> {
    let enabled: Vec<&&SemNode> = matches.iter().filter(|n| !n.disabled).collect();
    match enabled.len() {
        0 if matches.is_empty() => Err(SemanticError::NotFound(what)),
        0 => Err(SemanticError::Disabled(what)),
        1 => Ok(enabled[0].hit.center()),
        n => Err(SemanticError::Ambiguous { what, count: n }),
    }
}

/// Resolve the clickable center of the node with this role.
///
/// Returns screen coordinates only; the caller feeds them to real input. This
/// function never calls application controllers (Q07).
pub fn by_role(
    provider: &impl SemanticProvider,
    screen_revision: u64,
    role: &Role,
) -> Result<(u16, u16), SemanticError> {
    check_fresh(provider, screen_revision)?;
    let what = format!("role {role:?}");
    let matches: Vec<&SemNode> = provider
        .nodes()
        .iter()
        .filter(|n| &n.role == role)
        .collect();
    pick(what, matches)
}

/// Resolve the clickable center of the node with this id (same Q07 contract
/// as [`by_role`]).
pub fn by_id(
    provider: &impl SemanticProvider,
    screen_revision: u64,
    id: &str,
) -> Result<(u16, u16), SemanticError> {
    check_fresh(provider, screen_revision)?;
    let what = format!("id {id:?}");
    let matches: Vec<&SemNode> = provider
        .nodes()
        .iter()
        .filter(|n| n.id.as_deref() == Some(id))
        .collect();
    pick(what, matches)
}

/// Resolve the clickable center of the node with this label (same Q07
/// contract as [`by_role`]).
pub fn by_label(
    provider: &impl SemanticProvider,
    screen_revision: u64,
    label: &str,
) -> Result<(u16, u16), SemanticError> {
    check_fresh(provider, screen_revision)?;
    let what = format!("label {label:?}");
    let matches: Vec<&SemNode> = provider
        .nodes()
        .iter()
        .filter(|n| n.label.as_deref() == Some(label))
        .collect();
    pick(what, matches)
}

// ---------------------------------------------------------------------------
// RatatuiTestAdapter: example provider fed by test code.
// ---------------------------------------------------------------------------

/// Example [`SemanticProvider`] for Ratatui view tests.
///
/// Test code builds this alongside its draw closure, mapping widget areas to
/// nodes manually. Nothing is inferred from the rendered buffer.
#[derive(Debug, Clone, Default)]
pub struct RatatuiTestAdapter {
    revision: u64,
    nodes: Vec<SemNode>,
}

impl RatatuiTestAdapter {
    #[must_use]
    pub fn new(revision: u64) -> Self {
        Self {
            revision,
            nodes: Vec::new(),
        }
    }

    /// Record one node for a widget area the test laid out.
    pub fn push(&mut self, node: SemNode) {
        self.nodes.push(node);
    }

    /// Re-target the adapter at a newer screen revision (nodes must have been
    /// rebuilt for that revision by the test).
    pub fn set_revision(&mut self, revision: u64) {
        self.revision = revision;
        self.nodes.clear();
    }
}

impl SemanticProvider for RatatuiTestAdapter {
    fn revision(&self) -> u64 {
        self.revision
    }

    fn nodes(&self) -> &[SemNode] {
        &self.nodes
    }
}

// ---------------------------------------------------------------------------
// Harness: deterministic update/render + manual clock (Q09).
// ---------------------------------------------------------------------------

/// One input to [`Harness::update`]: clock movement or a scripted event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessEvent<E> {
    /// The manual clock advanced; payload is the new `now_ms`.
    Tick(u64),
    /// A scripted event fired.
    Event(E),
}

/// Deterministic runtime harness: caller-supplied `update` + `render`, a
/// manual millisecond clock, and a scripted event schedule.
///
/// No live clock, threads, or services. [`Harness::run`] renders the initial
/// state plus one [`Screen`] per scheduled event, in schedule order; identical
/// scripts produce identical screens.
pub struct Harness<S, E> {
    state: S,
    update: fn(&mut S, HarnessEvent<E>),
    render: for<'a> fn(&S, &mut ratatui::Frame<'a>),
    cols: u16,
    rows: u16,
    now_ms: u64,
    schedule: Vec<(u64, E)>,
    policy: EdgePolicy,
}

impl<S, E> Harness<S, E> {
    pub fn new(
        state: S,
        cols: u16,
        rows: u16,
        update: fn(&mut S, HarnessEvent<E>),
        render: for<'a> fn(&S, &mut ratatui::Frame<'a>),
    ) -> Self {
        Self {
            state,
            update,
            render,
            cols,
            rows,
            now_ms: 0,
            schedule: Vec::new(),
            policy: EdgePolicy::default(),
        }
    }

    /// Script one event at an absolute manual-clock time.
    pub fn schedule(&mut self, at_ms: u64, event: E) {
        self.schedule.push((at_ms, event));
    }

    /// Current manual-clock time.
    #[must_use]
    pub fn now(&self) -> u64 {
        self.now_ms
    }

    #[must_use]
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Move the clock forward by `ms`, firing due scripted events through
    /// `update` (a [`HarnessEvent::Tick`] first, then each due
    /// [`HarnessEvent::Event`] in schedule order). Returns the new time.
    pub fn advance(&mut self, ms: u64) -> u64 {
        self.now_ms += ms;
        let now = self.now_ms;
        (self.update)(&mut self.state, HarnessEvent::Tick(now));
        let mut i = 0;
        while i < self.schedule.len() {
            if self.schedule[i].0 <= now {
                let (_, ev) = self.schedule.remove(i);
                (self.update)(&mut self.state, HarnessEvent::Event(ev));
            } else {
                i += 1;
            }
        }
        now
    }

    /// Render the current state to a validated [`Screen`].
    pub fn screen(&self) -> Screen {
        let state = &self.state;
        let render = self.render;
        render_screen(self.cols, self.rows, |f| render(state, f), self.policy)
            .expect("harness render must produce a valid screen")
            .into_screen()
    }

    /// Run the whole script deterministically: initial screen plus one screen
    /// per scheduled event, in `(time, insertion)` order. Consumes the
    /// schedule; the clock ends at the last event time (or 0 when empty).
    pub fn run(mut self) -> Vec<Screen> {
        // Stable sort keeps insertion order within a timestamp.
        let mut times: Vec<u64> = self.schedule.iter().map(|(t, _)| *t).collect();
        times.sort();
        let mut out = Vec::with_capacity(times.len() + 1);
        out.push(self.screen());
        for at in times {
            let delta = at.saturating_sub(self.now_ms);
            // Entries sharing a timestamp fire together on the first step
            // that reaches them; later same-time steps render unchanged.
            self.advance(delta);
            out.push(self.screen());
        }
        out
    }
}
