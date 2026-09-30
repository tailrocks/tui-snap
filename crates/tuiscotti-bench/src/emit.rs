//! Raw per-sample JSONL writer. One line per sample, sorted keys.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// One benchmark sample: timing + RSS + verdict.
#[derive(Debug, Clone)]
pub struct Sample {
    /// Harness binary (`views` / `pty`).
    pub suite: &'static str,
    /// Scenario within the suite (`canonical`, `full`, ...).
    pub scenario: &'static str,
    /// Screen size label (`80x24`, ...).
    pub size: &'static str,
    /// Fixture journey (`plain`, `dense`, ...).
    pub journey: &'static str,
    /// Compare case (`equal`, `changed`, `corrupt`, `missing`, `-`).
    pub case: &'static str,
    /// Cache state (`warm-shared`, `no-cache`, ...).
    pub cache: &'static str,
    /// Worker index (0 when single-threaded).
    pub worker: u32,
    /// Iteration index within this worker.
    pub iter: u32,
    /// Wall time of the measured operation, nanoseconds.
    pub elapsed_ns: u128,
    /// Sampled process peak RSS after the operation (0 when unavailable).
    pub rss: u64,
    /// Unit of `rss` (`kibibytes` on Linux, `unknown` where unavailable).
    pub rss_units: &'static str,
    /// True when the operation's own correctness check held.
    pub ok: bool,
    /// Extra key=value facts (`spawn_ns=…`, `pixels_equal=true`, ...).
    pub detail: String,
}

impl Sample {
    /// Render as one JSON object line (sorted keys via `json!`).
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "cache": self.cache,
            "case": self.case,
            "detail": self.detail,
            "elapsed_ns": self.elapsed_ns,
            "iter": self.iter,
            "journey": self.journey,
            "ok": self.ok,
            "rss": self.rss,
            "rss_units": self.rss_units,
            "scenario": self.scenario,
            "size": self.size,
            "suite": self.suite,
            "worker": self.worker,
        })
        .to_string()
    }
}

/// One-line Debug prefix, capped so a failure detail never embeds a whole
/// screen dump (wait/teardown errors carry full observations).
#[must_use]
pub fn short_debug(value: &dyn std::fmt::Debug) -> String {
    let text = format!("{value:?}");
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 240 {
        flat
    } else {
        format!("{}…", flat.chars().take(240).collect::<String>())
    }
}

/// Append-only JSONL sink (single-threaded use; workers send lines back).
#[derive(Debug)]
pub struct Sink {
    out: BufWriter<File>,
    count: u64,
}

impl Sink {
    /// Create (truncate) the JSONL file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be created.
    pub fn create(path: &Path) -> anyhow::Result<Self> {
        let file =
            File::create(path).map_err(|e| anyhow::anyhow!("create {}: {e}", path.display()))?;
        Ok(Self {
            out: BufWriter::new(file),
            count: 0,
        })
    }

    /// Write one sample line.
    ///
    /// # Errors
    ///
    /// Returns an error when the sample cannot be written.
    pub fn write(&mut self, sample: &Sample) -> anyhow::Result<()> {
        writeln!(self.out, "{}", sample.to_json())
            .map_err(|e| anyhow::anyhow!("write sample: {e}"))?;
        self.count += 1;
        Ok(())
    }

    /// Flush and return the sample count.
    ///
    /// # Errors
    ///
    /// Returns an error when the buffer cannot be flushed.
    pub fn finish(mut self) -> anyhow::Result<u64> {
        self.out
            .flush()
            .map_err(|e| anyhow::anyhow!("flush samples: {e}"))?;
        Ok(self.count)
    }
}
