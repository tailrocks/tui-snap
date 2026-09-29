//! Semantic provider + harness tests (backlog Q06, Q07, Q09).

use tuiscotti::ratatui as tr;
use tuiscotti::semant::{
    by_id, by_label, by_role, Harness, HarnessEvent, HitRegion, RatatuiTestAdapter, Role, SemNode,
    SemanticError, SemanticProvider,
};

fn hit(x: u16, y: u16, cols: u16, rows: u16) -> HitRegion {
    HitRegion { x, y, cols, rows }
}

fn adapter() -> RatatuiTestAdapter {
    let mut a = RatatuiTestAdapter::new(7);
    a.push(
        SemNode::new(Role::Button, hit(2, 1, 10, 3))
            .with_id("ok")
            .with_label("OK"),
    );
    a.push(
        SemNode::new(Role::Textbox, hit(2, 5, 20, 1))
            .with_id("name")
            .with_label("Name")
            .focused(),
    );
    a.push(
        SemNode::new(Role::Button, hit(14, 1, 10, 3))
            .with_id("cancel")
            .with_label("Cancel")
            .disabled(),
    );
    a
}

#[test]
fn roles_ids_labels_focus_disabled() {
    let a = adapter();
    assert_eq!(a.revision(), 7);
    assert_eq!(a.nodes().len(), 3);
    assert!(a.nodes()[1].focused);
    assert!(!a.nodes()[0].focused);
    assert!(a.nodes()[2].disabled);
    assert!(!a.nodes()[0].disabled);
    // Locators hit provider data, not pixels: enabled button resolves to its
    // hit-region center (2+10/2, 1+3/2) = (7,2).
    assert_eq!(by_id(&a, 7, "ok"), Ok((7, 2)));
    assert_eq!(by_label(&a, 7, "Name"), Ok((12, 5)));
    assert_eq!(by_role(&a, 7, &Role::Textbox), Ok((12, 5)));
}

#[test]
fn hit_region_center_coords() {
    // Odd dims: (0,0,5,1) -> (2,0). Even dims round down: (0,0,4,2) -> (2,1).
    assert_eq!(hit(0, 0, 5, 1).center(), (2, 0));
    assert_eq!(hit(10, 4, 4, 2).center(), (12, 5));
}

#[test]
fn disabled_excluded_from_click() {
    let a = adapter();
    // The only "Cancel" node is disabled -> Disabled, never coords.
    assert_eq!(
        by_id(&a, 7, "cancel"),
        Err(SemanticError::Disabled("id \"cancel\"".to_string()))
    );
    assert_eq!(
        by_label(&a, 7, "Cancel"),
        Err(SemanticError::Disabled("label \"Cancel\"".to_string()))
    );
    // Role Button matches one enabled + one disabled: disabled never wins.
    assert_eq!(by_role(&a, 7, &Role::Button), Ok((7, 2)));
}

#[test]
fn ambiguity_and_not_found() {
    let mut a = RatatuiTestAdapter::new(1);
    a.push(SemNode::new(Role::Link, hit(0, 0, 4, 1)).with_label("docs"));
    a.push(SemNode::new(Role::Link, hit(5, 0, 4, 1)).with_label("docs"));
    assert_eq!(
        by_label(&a, 1, "docs"),
        Err(SemanticError::Ambiguous {
            what: "label \"docs\"".to_string(),
            count: 2,
        })
    );
    assert_eq!(
        by_id(&a, 1, "nope"),
        Err(SemanticError::NotFound("id \"nope\"".to_string()))
    );
}

#[test]
fn stale_generation_refused() {
    let a = adapter(); // provider revision 7
    assert_eq!(
        by_id(&a, 8, "ok"),
        Err(SemanticError::Stale {
            screen_revision: 8,
            provider_revision: 7,
        })
    );
    assert_eq!(
        by_role(&a, 6, &Role::Button),
        Err(SemanticError::Stale {
            screen_revision: 6,
            provider_revision: 7,
        })
    );
    // Re-targeting clears stale nodes; locator works again after rebuild.
    let mut b = adapter();
    b.set_revision(8);
    assert_eq!(
        by_id(&b, 8, "ok"),
        Err(SemanticError::NotFound("id \"ok\"".to_string()))
    );
}

#[test]
fn resolution_yields_coords_only() {
    // Q07: locators return plain (x,y) for real input; they take only
    // &provider + revision + key, so no controller call is possible. The
    // type itself is the proof: a bare tuple, no handle, no closure.
    let a = adapter();
    let coords: (u16, u16) = by_id(&a, 7, "ok").unwrap();
    assert_eq!(coords, (7, 2));
    // Same call twice, no side effects on provider.
    assert_eq!(by_id(&a, 7, "ok").unwrap(), coords);
    assert_eq!(a.nodes().len(), 3);
}

#[test]
fn no_appearance_inference() {
    // Same pixels, different semantics -> different resolution. Both adapters
    // describe an identical 20x3 screen, but node data differs; the locator
    // follows the data, never the cells.
    let screen = tr::render_screen(
        20,
        3,
        |f| {
            use ratatui::widgets::{Block, Borders, Paragraph};
            f.render_widget(
                Paragraph::new("OK").block(Block::default().borders(Borders::ALL)),
                f.area(),
            );
        },
        tr::EdgePolicy::default(),
    )
    .unwrap()
    .into_screen();
    let _ = screen; // pixels exist but locators never read them.

    let mut left = RatatuiTestAdapter::new(0);
    left.push(SemNode::new(Role::Button, hit(0, 0, 10, 3)).with_id("action"));
    let mut right = RatatuiTestAdapter::new(0);
    right.push(SemNode::new(Role::Button, hit(10, 0, 10, 3)).with_id("action"));
    assert_eq!(by_id(&left, 0, "action"), Ok((5, 1)));
    assert_eq!(by_id(&right, 0, "action"), Ok((15, 1)));

    // And a provider with no nodes resolves nothing even though the screen
    // visibly contains an "OK"-looking button: no guessing from styling.
    let empty = RatatuiTestAdapter::new(0);
    assert!(matches!(
        by_role(&empty, 0, &Role::Button),
        Err(SemanticError::NotFound(_))
    ));
}

// ---------------------------------------------------------------------------
// Harness (Q09).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ev {
    Inc,
    Dec,
}

fn update(state: &mut i32, ev: HarnessEvent<Ev>) {
    match ev {
        HarnessEvent::Tick(_) => {}
        HarnessEvent::Event(Ev::Inc) => *state += 1,
        HarnessEvent::Event(Ev::Dec) => *state -= 1,
    }
}

fn render(state: &i32, f: &mut ratatui::Frame<'_>) {
    use ratatui::widgets::Paragraph;
    f.render_widget(Paragraph::new(format!("n={state}")), f.area());
}

fn script() -> Harness<i32, Ev> {
    let mut h = Harness::new(0, 20, 3, update, render);
    h.schedule(10, Ev::Inc);
    h.schedule(20, Ev::Inc);
    h.schedule(30, Ev::Dec);
    h
}

#[test]
fn harness_determinism() {
    let a = script().run();
    let b = script().run();
    // One screen per step: initial + 3 events.
    assert_eq!(a.len(), 4);
    assert_eq!(b.len(), 4);
    assert_eq!(a, b);
    // States evolved 0 -> 1 -> 2 -> 1; each screen pins its step.
    assert!(a[0].cells().iter().any(|c| c.symbol == "0"));
    assert!(a[1].cells().iter().any(|c| c.symbol == "1"));
    assert!(a[2].cells().iter().any(|c| c.symbol == "2"));
    assert!(a[3].cells().iter().any(|c| c.symbol == "1"));
}

#[test]
fn harness_uses_update_render_not_view_capture() {
    // Pure view capture renders a fixed closure; the harness threads caller
    // update/render fns and a manual clock instead. Proof: advancing the
    // clock with no events still delivers Tick (observable via state), and
    // screens track reducer state, not a static draw.
    fn tick_counter(state: &mut u64, ev: HarnessEvent<Ev>) {
        if matches!(ev, HarnessEvent::Tick(_)) {
            *state += 1;
        }
    }
    fn render_tick(state: &u64, f: &mut ratatui::Frame<'_>) {
        use ratatui::widgets::Paragraph;
        f.render_widget(Paragraph::new(format!("t={state}")), f.area());
    }
    let mut h = Harness::new(0u64, 20, 3, tick_counter, render_tick);
    assert_eq!(h.now(), 0);
    h.advance(100);
    assert_eq!(h.now(), 100);
    assert_eq!(*h.state(), 1); // Tick observed: reducer ran, not just a view.
    let s = h.screen();
    assert!(s.cells().iter().any(|c| c.symbol == "1"));

    // Out-of-order scheduling still runs in time order deterministically.
    let mut h2 = Harness::new(0, 20, 3, update, render);
    h2.schedule(30, Ev::Dec);
    h2.schedule(10, Ev::Inc);
    h2.schedule(20, Ev::Inc);
    assert_eq!(h2.run(), script().run());
}
