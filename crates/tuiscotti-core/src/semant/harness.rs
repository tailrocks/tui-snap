//! Deterministic `update`/`render` + manual clock harness for runtime tests
//! (Q09). No live clock, threads, or services.

use crate::ratatui::{EdgePolicy, render_screen};
use crate::screen::Screen;

/// One input to `Harness::update`: clock movement or a scripted event.
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
#[derive(Debug)]
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
    /// Harness over `state` with an empty schedule at clock 0.
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

    /// Borrowed harness state.
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
    ///
    /// Total: under [`EdgePolicy::ClipWithReplacement`] the render fails only
    /// on invalid dimensions, which [`Screen::blank`] re-asserts loudly
    /// instead of hiding the failure behind an empty grid.
    pub fn screen(&self) -> Screen {
        let state = &self.state;
        let render = self.render;
        match render_screen(self.cols, self.rows, |f| render(state, f), self.policy) {
            Ok(capture) => capture.into_screen(),
            Err(_) => Screen::blank(self.cols, self.rows),
        }
    }

    /// Run the whole script deterministically: initial screen plus one screen
    /// per scheduled event, in `(time, insertion)` order. Consumes the
    /// schedule; the clock ends at the last event time (or 0 when empty).
    pub fn run(mut self) -> Vec<Screen> {
        // Bare timestamps carry no payload, so order among equals is moot.
        let mut times: Vec<u64> = self.schedule.iter().map(|(t, _)| *t).collect();
        times.sort_unstable();
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
