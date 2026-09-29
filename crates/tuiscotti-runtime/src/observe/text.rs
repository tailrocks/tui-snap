use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use tuiscotti_core::screen::Screen;

// ---------------------------------------------------------------------------
// Shared helpers (feature-independent)
// ---------------------------------------------------------------------------

/// Plain-text rows of a screen, one line per row, trailing blanks trimmed.
///
/// This is a lossy human projection (colors/mods/cursor dropped): fine for a
/// best-effort live view, never an assertion input.
#[must_use]
pub fn screen_text(screen: &Screen) -> String {
    let mut out = String::new();
    for y in 0..screen.rows() {
        let mut row = String::new();
        for x in 0..screen.cols() {
            if let Some(c) = screen.get(x, y)
                && !c.continuation
            {
                row.push_str(&c.symbol);
            }
        }
        if y > 0 {
            out.push('\n');
        }
        out.push_str(row.trim_end());
    }
    out
}

fn screen_hash(screen: &Screen) -> u64 {
    let mut h = DefaultHasher::new();
    screen.hash(&mut h);
    h.finish()
}

/// Same/different verdict for [`compare_replay_vs_rerun`], with revision maps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRerunComparison {
    /// True when the final replayed screen equals the re-run screen.
    pub same: bool,
    /// Screen hash per replayed prefix, in prefix order (replay revision map).
    pub replay_hashes: Vec<u64>,
    /// Screen hash of the re-run's final observation.
    pub rerun_hash: u64,
    /// Human-readable verdict detail.
    pub detail: String,
}

/// Compare replayed screens against a re-run screen.
///
/// `replay_screens` is the per-output-prefix screens from
/// [`Recording::replay_observations`](crate::tui_shell::Recording) (or any
/// replay path); only the final screen is compared, the full prefix list is
/// kept as the revision map. Equality is exact [`Screen`] equality.
#[must_use]
pub fn compare_replay_vs_rerun(
    replay_screens: &[Screen],
    rerun_screen: &Screen,
) -> ReplayRerunComparison {
    let replay_hashes: Vec<u64> = replay_screens.iter().map(screen_hash).collect();
    let rerun_hash = screen_hash(rerun_screen);
    let same = replay_screens.last().is_some_and(|s| s == rerun_screen);
    let detail = match replay_screens.last() {
        None => format!("no replayed screens; rerun hash {rerun_hash:016x}: DIFFERENT"),
        Some(_) if same => format!(
            "final replay screen == rerun screen ({} prefixes, hash {rerun_hash:016x}): SAME",
            replay_screens.len()
        ),
        Some(_) => format!(
            "final replay screen != rerun screen ({} prefixes, replay {:016x} vs rerun {rerun_hash:016x}): DIFFERENT",
            replay_screens.len(),
            replay_hashes.last().copied().unwrap_or(0),
        ),
    };
    ReplayRerunComparison {
        same,
        replay_hashes,
        rerun_hash,
        detail,
    }
}
