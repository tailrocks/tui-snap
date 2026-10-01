//! Flood/coalescing regression (F12): a saturating flood through the
//! worker/session path stays revision-bounded and drains on time.
//!
//! Debug-gated by measurement, not convenience: in release the worker
//! outruns any PTY producer, so no structural backlog forms and the count
//! cannot pin coalescing (measured 1346/1924/2112 across identical release
//! runs — straddling the 776 bound). In debug the worker is structurally
//! slower than the bulk producer, the backlog must coalesce, and the
//! limit-derived bound below bites any removal (~280 observed vs ~16k
//! uncoalesced — short reads multiply the batches). The project gates
//! run debug.

#![cfg(feature = "pty")]
#![cfg(debug_assertions)]
#![cfg(unix)]

use std::time::{Duration, Instant};

use tuiscotti_runtime::tui::{CancelToken, Tui};

/// Flood volume: 16 MiB of plain-text lines through the PTY.
const FLOOD_BYTES: u64 = 16 * 1024 * 1024;
/// Reader batch size (`run_reader` in `tui/capture.rs`): without coalescing
/// this flood costs one revision per batch.
const READER_BATCH: u64 = 8 * 1024;
/// Coalescing cap (`COALESCE_BYTES` in `tui/limits.rs`): pending batches
/// merge into one emulator advance per cap under a saturating flood.
const COALESCE_BYTES: u64 = 256 * 1024;
/// Op-channel depth (`OP_QUEUE_LIMIT` in `tui/limits.rs`): bounds the
/// startup/drain transients where the worker outruns the reader.
const OP_QUEUE_LIMIT: u64 = 64;
/// Slack over the saturated ideal: partially-filled advances under
/// scheduling jitter (observed ~280 in debug; one-revision-per-full-batch
/// is 2048, so this still pins ~2.6x better than no coalescing at all).
const SLACK: u64 = 10;

/// Principled revision bound: one saturated advance per coalescing cap
/// times slack, plus transients bounded by twice the queue depth, plus
/// initial/final revisions. Every term derives from the flood volume or a
/// limit — no magic snapshot of today's count.
const REVISION_BOUND: u64 = FLOOD_BYTES.div_ceil(COALESCE_BYTES) * SLACK + 2 * OP_QUEUE_LIMIT + 8;

/// Pre-fix (uncoalesced) scale from the F12 commit message: ~31k revisions
/// for its flood. The bound must sit far below that scale or it pins
/// nothing.
const PREFIX_REVISIONS: u64 = 31_000;

/// Bounded drain deadline: the old-backend calibration (a 248 MB flood
/// draining in 0.22 s) died with the termpane swap — the new emulator parses
/// ~5x slower in debug (measured ~59 s alone / ~63 s loaded for this 16 MiB
/// flood on a dev mac; the parse path, not the adapter, dominates). The
/// ceiling is recalibrated to 300 s: still a real bound (a hung drain fails
/// instead of hanging the suite) with headroom for loaded CI. The revision
/// bound above is the structural pin and is unchanged.
const DRAIN_DEADLINE: Duration = Duration::from_secs(300);

#[test]
fn saturating_flood_stays_revision_bounded_and_drains() {
    const {
        assert!(
            REVISION_BOUND < PREFIX_REVISIONS,
            "bound must sit far below the pre-fix scale"
        );
    }
    // Bulk producer: `cat` of a 16 MiB text file dumps 8 KiB reader batches
    // as fast as the PTY accepts them, so the worker (one grid build per
    // advance) structurally cannot keep up batch-for-batch and the backlog
    // must coalesce. A line-paced producer (`yes`, `seq`) would let the
    // worker keep up and pin nothing about the limits.
    let line: String = "0123456789abcdef".repeat(4) + "\n";
    let repeats = usize::try_from(FLOOD_BYTES / line.len() as u64).expect("flood fits in memory");
    let target_len = usize::try_from(FLOOD_BYTES).expect("flood fits in memory");
    let mut body = line.repeat(repeats);
    while body.len() < target_len {
        body.push('x');
    }
    assert_eq!(body.len(), target_len, "exact flood volume");
    let file = std::env::temp_dir().join(format!("tuisnap-flood-{}", std::process::id()));
    std::fs::write(&file, &body).expect("stage flood file");
    let cmd = format!("stty -opost; cat '{}'", file.display());
    let s = Tui::new(["/bin/sh", "-c", cmd.as_str()])
        .size(80, 24)
        .spawn()
        .expect("spawn succeeds");
    let cancel = CancelToken::new();
    let start = Instant::now();
    s.wait_exit(start + DRAIN_DEADLINE, &cancel)
        .expect("flood child exits within the deadline");
    s.wait_stable(start + DRAIN_DEADLINE, &cancel)
        .expect("drain settles within the deadline");
    let elapsed = start.elapsed();
    let revs = s.revision();
    assert!(
        revs <= REVISION_BOUND,
        "flood cost {revs} revisions, bound {REVISION_BOUND} \
         (full-size batches: {}; short reads add more)",
        FLOOD_BYTES.div_ceil(READER_BATCH)
    );
    assert!(
        elapsed < DRAIN_DEADLINE,
        "drain took {elapsed:?}, deadline {DRAIN_DEADLINE:?}"
    );
    s.close().expect("close succeeds");
    if std::fs::remove_file(&file).is_err() {
        // Best-effort staging cleanup; /tmp is scratch.
    }
}
