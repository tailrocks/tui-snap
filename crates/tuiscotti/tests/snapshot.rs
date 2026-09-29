//! Approved-store behaviors: write-before-assert, explicit accept,
//! corrupt/missing handling, concurrency, reports, no auto-bless.

use ratatui::widgets::Paragraph;
use tuiscotti::snapshot::Store;
use tuiscotti::{Profile, Provenance};

#[path = "snapshot/lifecycle.rs"]
mod lifecycle;
#[path = "snapshot/safety.rs"]
mod safety;
#[path = "snapshot/store.rs"]
mod store;

fn prov() -> Provenance {
    Provenance {
        tool: "tuisnap".into(),
        tool_version: "test".into(),
        profile: "tuisnap-default".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    }
}

fn profile() -> Profile {
    Profile::default_profile()
}

fn frame_with(text: &str) -> tuiscotti::Frame {
    tuiscotti::ratatui::widget_frame(Paragraph::new(text), 30, 6, prov())
}

fn tmp_store(tag: &str) -> Result<(tempfile::TempDir, Store), String> {
    let dir = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
    let st = Store::new(&dir.path().join(tag));
    Ok((dir, st))
}
