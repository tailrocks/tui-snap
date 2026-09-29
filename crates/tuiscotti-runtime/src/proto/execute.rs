use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use super::*;


// ---------------------------------------------------------------------------
// execute(): the library entry
// ---------------------------------------------------------------------------

/// Execute one op against the process-local session registry.
pub fn execute(op: &Op) -> Result<OpResult, OpError> {
    match op {
        Op::Version => Ok(OpResult::Version {
            protocol: PROTOCOL_VERSION.to_string(),
            tuisnap: env!("CARGO_PKG_VERSION").to_string(),
        }),
        Op::Capabilities => Ok(OpResult::Capabilities {
            capabilities: capabilities(),
        }),
        Op::Assert {
            check,
            text,
            needle,
            actual,
            expected,
        } => execute_assert(check, text, needle, actual, expected),
        Op::Render { frame_json, format } => execute_render(frame_json, format),
        Op::Diff {
            expected_png_b64,
            actual_png_b64,
        } => execute_diff(expected_png_b64, actual_png_b64),
        Op::SessionStart { name, argv, force } => Ok(OpResult::Session {
            session: session_start(name, argv, *force)?,
        }),
        Op::SessionStop { name } => Ok(OpResult::Session {
            session: session_stop(name)?,
        }),
        Op::SessionList => Ok(OpResult::SessionList {
            sessions: session_list()?,
        }),
        #[cfg(feature = "pty")]
        Op::Spawn {
            argv,
            id,
            cols,
            rows,
            cwd,
            env,
        } => pty_registry::spawn(argv, id.clone(), *cols, *rows, cwd.clone(), env),
        #[cfg(feature = "pty")]
        Op::Stdin {
            session,
            text,
            chord,
            bytes_b64,
        } => pty_registry::stdin(session, text.clone(), chord.clone(), bytes_b64.clone()),
        #[cfg(feature = "pty")]
        Op::Observe { session } => pty_registry::observe(session),
        #[cfg(feature = "pty")]
        Op::Snapshot { session } => pty_registry::snapshot(session),
        #[cfg(feature = "pty")]
        Op::Screenshot { session } => pty_registry::screenshot(session),
        #[cfg(feature = "pty")]
        Op::Wait {
            session,
            kind,
            needle,
            quiet_ms,
            timeout_ms,
        } => pty_registry::wait(session, kind, needle.clone(), *quiet_ms, *timeout_ms),
        #[cfg(feature = "pty")]
        Op::Exit {
            session,
            timeout_ms,
        } => pty_registry::exit(session, *timeout_ms),
        #[cfg(not(feature = "pty"))]
        Op::Spawn { .. }
        | Op::Stdin { .. }
        | Op::Observe { .. }
        | Op::Snapshot { .. }
        | Op::Screenshot { .. }
        | Op::Wait { .. }
        | Op::Exit { .. } => Err(OpError::new(
            "unsupported",
            "PTY ops need the `pty` feature",
        )),
    }
}


fn execute_assert(
    check: &str,
    text: &Option<String>,
    needle: &Option<String>,
    actual: &Option<String>,
    expected: &Option<String>,
) -> Result<OpResult, OpError> {
    fn need<'a>(v: &'a Option<String>, check: &str, what: &str) -> Result<&'a str, OpError> {
        v.as_deref()
            .ok_or_else(|| OpError::new("invalid-input", format!("assert {check} needs `{what}`")))
    }
    match check {
        assert_check::TEXT_CONTAINS => {
            let text = need(text, check, "text")?;
            let needle = need(needle, check, "needle")?;
            if needle.is_empty() {
                return Err(OpError::new("invalid-input", "needle must not be empty"));
            }
            let passed = text.contains(needle);
            Ok(OpResult::Asserted {
                passed,
                detail: if passed {
                    format!("text contains {needle:?}")
                } else {
                    format!("text ({} chars) lacks {needle:?}", text.len())
                },
            })
        }
        assert_check::TEXT_EQUALS => {
            let actual = need(actual, check, "actual")?;
            let expected = need(expected, check, "expected")?;
            let passed = actual == expected;
            Ok(OpResult::Asserted {
                passed,
                detail: if passed {
                    "texts equal".to_string()
                } else {
                    format!(
                        "lengths differ: actual {} vs expected {}",
                        actual.len(),
                        expected.len()
                    )
                },
            })
        }
        other => Err(OpError::new(
            "invalid-input",
            format!("unknown assert check {other:?} (want text-contains|text-equals)"),
        )),
    }
}


fn execute_render(frame_json: &str, format: &str) -> Result<OpResult, OpError> {
    use tuiscotti_render::profile::VENDORED_FACES;
    use tuiscotti_render::render::Renderer;
    let frame = tuiscotti_core::frame::Frame::from_json(frame_json)
        .map_err(|e| OpError::new("invalid-input", format!("bad frame JSON: {e}")))?;
    let profile = tuiscotti_render::profile::Profile::default_profile();
    let mut renderer = Renderer::new(&profile, &VENDORED_FACES)
        .map_err(|e| OpError::new("render", e.to_string()))?;
    match format {
        "png" => {
            let rendered = renderer
                .render(&frame)
                .map_err(|e| OpError::new("render", e.to_string()))?;
            Ok(OpResult::Rendered {
                format: format.to_string(),
                data: base64_encode(&rendered.png),
                data_b64: true,
            })
        }
        "ansi" => Ok(OpResult::Rendered {
            format: format.to_string(),
            data: tuiscotti_render::render::ansi_dump(&frame),
            data_b64: false,
        }),
        "txt" => Ok(OpResult::Rendered {
            format: format.to_string(),
            data: frame.text(),
            data_b64: false,
        }),
        "svg" => Ok(OpResult::Rendered {
            format: format.to_string(),
            data: tuiscotti_render::render::render_svg(&frame, &profile),
            data_b64: false,
        }),
        "html" => {
            let html = renderer
                .render_html(&frame, "frame")
                .map_err(|e| OpError::new("render", e.to_string()))?;
            Ok(OpResult::Rendered {
                format: format.to_string(),
                data: html,
                data_b64: false,
            })
        }
        other => Err(OpError::new(
            "invalid-input",
            format!("unknown format {other:?} (want png|ansi|txt|svg|html)"),
        )),
    }
}


fn execute_diff(expected_b64: &str, actual_b64: &str) -> Result<OpResult, OpError> {
    let expected = base64_decode(expected_b64)
        .map_err(|e| OpError::new("invalid-input", format!("bad expected PNG base64: {e}")))?;
    let actual = base64_decode(actual_b64)
        .map_err(|e| OpError::new("invalid-input", format!("bad actual PNG base64: {e}")))?;
    let verdict = tuiscotti_render::diff::compare_png(&expected, &actual)
        .map_err(|e| OpError::new("invalid-input", format!("PNG compare failed: {e}")))?;
    Ok(OpResult::Diffed {
        pixels_equal: verdict.pixels_equal,
        dims_equal: verdict.dims_equal,
        score: verdict.score,
    })
}


// ---------------------------------------------------------------------------
// Screen projections
// ---------------------------------------------------------------------------

/// Plain-text projection of a screen: symbols row by row, continuations
/// skipped, trailing whitespace trimmed per row.
#[must_use]
pub fn screen_text(screen: &tuiscotti_core::screen::Screen) -> String {
    let mut out = String::new();
    for y in 0..screen.rows() {
        if y > 0 {
            out.push('\n');
        }
        let mut row = String::new();
        for x in 0..screen.cols() {
            if let Some(c) = screen.get(x, y) {
                if !c.continuation {
                    row.push_str(&c.symbol);
                }
            }
        }
        while row.ends_with([' ', '\t']) {
            row.pop();
        }
        out.push_str(&row);
    }
    out
}


#[must_use]
pub fn screen_view(screen: &tuiscotti_core::screen::Screen) -> ScreenView {
    let cursor = screen.cursor();
    ScreenView {
        cols: screen.cols(),
        rows: screen.rows(),
        text: screen_text(screen),
        cursor_x: cursor.x,
        cursor_y: cursor.y,
        cursor_visible: cursor.visible,
    }
}


#[must_use]
pub fn observation_view(obs: &tuiscotti_core::screen::Observation) -> ObservationView {
    ObservationView {
        revision: obs.revision,
        reason: format!("{:?}", obs.reason),
        screen: screen_view(&obs.screen),
    }
}


// ---------------------------------------------------------------------------
// Base64 (no new deps: small local implementation over base64 crate)
// ---------------------------------------------------------------------------

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}


pub(crate) fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| e.to_string())
}
