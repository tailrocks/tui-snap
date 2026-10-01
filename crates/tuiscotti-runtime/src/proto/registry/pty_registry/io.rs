//! Registry I/O ops: input, observe, snapshot, screenshot.
//!
//! Split out of `registry.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use crate::proto::{
    OpError, OpResult, base64_decode, base64_encode, observation_view, screen_view,
};

use super::{lookup, tui_err};

pub(crate) fn stdin(
    session: &str,
    text: Option<String>,
    chord: Option<String>,
    bytes_b64: Option<String>,
) -> Result<OpResult, OpError> {
    let set = [text.is_some(), chord.is_some(), bytes_b64.is_some()]
        .into_iter()
        .filter(|b| *b)
        .count();
    if set != 1 {
        return Err(OpError::new(
            "invalid-input",
            "stdin needs exactly one of text|chord|bytes_b64",
        )
        .with_session(session));
    }
    let s = lookup(session)?;
    let r = if let Some(text) = text {
        if text.is_empty() {
            return Err(
                OpError::new("invalid-input", "text must not be empty").with_session(session)
            );
        }
        s.send_text(&text)
    } else if let Some(chord) = chord {
        s.press(&chord)
    } else if let Some(b64) = bytes_b64 {
        let bytes = base64_decode(&b64).map_err(|e| {
            OpError::new("invalid-input", format!("bad bytes_b64: {e}")).with_session(session)
        })?;
        if bytes.is_empty() {
            return Err(
                OpError::new("invalid-input", "bytes must not be empty").with_session(session)
            );
        }
        s.send_bytes(&bytes)
    } else {
        unreachable!("counted above");
    };
    r.map_err(|e| tui_err(&e).with_session(session))?;
    Ok(OpResult::InputAccepted {
        session: session.to_string(),
    })
}

pub(crate) fn observe(session: &str) -> Result<OpResult, OpError> {
    let s = lookup(session)?;
    let obs = s
        .observe_now()
        .map_err(|e| tui_err(&e).with_session(session))?;
    Ok(OpResult::Observation {
        observation: observation_view(&obs),
    })
}

pub(crate) fn snapshot(session: &str) -> Result<OpResult, OpError> {
    let s = lookup(session)?;
    let screen = s
        .snapshot()
        .map_err(|e| tui_err(&e).with_session(session))?;
    Ok(OpResult::Snapshot {
        screen: screen_view(&screen),
    })
}

pub(crate) fn screenshot(session: &str) -> Result<OpResult, OpError> {
    let s = lookup(session)?;
    let obs = s
        .observe_now()
        .map_err(|e| tui_err(&e).with_session(session))?;
    let canonical = tuiscotti_core::screen::canonical_string(&obs.screen);
    let profile = tuiscotti_render::profile::Profile::default_profile();
    // Shared default renderer (F12): faces parsed once per thread,
    // glyph cache shared across screenshots.
    let image = tuiscotti_render::render::Renderer::with_profile(
        &profile,
        &tuiscotti_render::profile::VENDORED_FACES,
        |r| r.render_screen(&obs.screen),
    )
    .map_err(|e| OpError::new("render", e.to_string()).with_session(session))?;
    Ok(OpResult::Screenshot {
        screen: screen_view(&obs.screen),
        canonical,
        png_b64: base64_encode(&image.png),
    })
}
