//! Semantic provider + locators (backlog Q06, Q07).
//!
//! [`SemanticProvider`] carries explicit role/id/label/focused/disabled/hit-region
//! data. Semantics NEVER come from appearance: nothing here inspects pixels,
//! glyphs, styles, or [`Screen`](crate::screen::Screen) cells. Locators
//! ([`by_role`], [`by_id`], [`by_label`]) resolve provider nodes to
//! hit-region-center screen coordinates for REAL input. They return coords
//! only and never call application controllers (Q07).
//! [`RatatuiTestAdapter`] is the example provider fed by test code alongside a
//! draw closure; tests map widget areas to nodes manually.

/// Widget role. Fixed set; providers needing more map them onto these or use
/// [`Role::Static`] for non-interactive text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Role {
    /// Activable push button.
    Button,
    /// Editable text field.
    Textbox,
    /// Toggleable checkbox.
    Checkbox,
    /// Selectable list entry.
    ListItem,
    /// Navigable link.
    Link,
    /// Non-interactive text.
    Static,
}

/// Hit region in screen grid coordinates (same space as
/// [`Screen`](crate::screen::Screen) cells). The locator clicks its center.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HitRegion {
    /// Grid column of the region start.
    pub x: u16,
    /// Grid row of the region start.
    pub y: u16,
    /// Region width in columns.
    pub cols: u16,
    /// Region height in rows.
    pub rows: u16,
}

impl HitRegion {
    /// Region center, the locator click point.
    #[must_use]
    pub fn center(&self) -> (u16, u16) {
        (self.x + self.cols / 2, self.y + self.rows / 2)
    }
}

/// One semantic node: explicit provider data only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemNode {
    /// Widget role.
    pub role: Role,
    /// Stable node id, when assigned.
    pub id: Option<String>,
    /// Visible label, when present.
    pub label: Option<String>,
    /// Whether the node holds focus.
    pub focused: bool,
    /// Disabled nodes are excluded from click targets.
    pub disabled: bool,
    /// Clickable screen region.
    pub hit: HitRegion,
}

impl SemNode {
    /// Enabled unfocused node without id or label.
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

    /// Attach a stable node id.
    #[must_use]
    pub fn with_id(mut self, id: &str) -> Self {
        self.id = Some(id.to_string());
        self
    }

    /// Attach a visible label.
    #[must_use]
    pub fn with_label(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    /// Mark the node focused.
    #[must_use]
    pub fn focused(mut self) -> Self {
        self.focused = true;
        self
    }

    /// Mark the node disabled (excluded from click targets).
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
    /// Screen revision the nodes were built for.
    fn revision(&self) -> u64;
    /// Explicit nodes; never inferred from rendering.
    fn nodes(&self) -> &[SemNode];
}

/// Locator resolution failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticError {
    /// No enabled node matched.
    NotFound(String),
    /// More than one enabled node matched; disambiguate.
    Ambiguous {
        /// What was matched.
        what: String,
        /// Number of enabled matches.
        count: usize,
    },
    /// The matched node is disabled: excluded from click targets.
    Disabled(String),
    /// Provider data is for another screen revision; re-capture first.
    Stale {
        /// Screen revision under test.
        screen_revision: u64,
        /// Revision the provider data was built for.
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
fn pick(what: String, matches: &[&SemNode]) -> Result<(u16, u16), SemanticError> {
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
///
/// # Errors
///
/// Returns [`SemanticError::Stale`] on revision mismatch,
/// [`SemanticError::NotFound`] on zero matches,
/// [`SemanticError::Ambiguous`] on 2+, or [`SemanticError::Disabled`] when
/// every match is disabled.
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
    pick(what, &matches)
}

/// Resolve the clickable center of the node with this id (same Q07 contract
/// as [`by_role`]).
///
/// # Errors
///
/// Same failures as [`by_role`].
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
    pick(what, &matches)
}

/// Resolve the clickable center of the node with this label (same Q07
/// contract as [`by_role`]).
///
/// # Errors
///
/// Same failures as [`by_role`].
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
    pick(what, &matches)
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
    /// Empty adapter targeting `revision`.
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
