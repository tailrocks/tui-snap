// ---------------------------------------------------------------------------
// A03 execution paths (need the PTY runtime)
// ---------------------------------------------------------------------------

#[cfg(feature = "pty")]
mod pty_paths {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use crate::tui::{Session, TuiError};
    use tuiscotti_core::screen::Observation;

    // -- A03: Watcher --------------------------------------------------------

    /// One delivered watch item: the observation plus the cumulative eviction
    /// count at delivery time.
    #[derive(Debug, Clone)]
    pub struct Watched {
        /// Exactly what assertions see: the live [`Observation`].
        pub observation: Observation,
        /// Total observations evicted before this one was delivered.
        pub dropped_before: u64,
    }

    struct WatcherInner {
        queue: Mutex<VecDeque<Observation>>,
        changed: Condvar,
        dropped: AtomicU64,
        stop: AtomicBool,
    }

    /// Live subscription to an owned [`Session`]'s observation stream.
    ///
    /// A poll thread tracks [`Session::revision`] and forwards each new
    /// [`Session::observe_now`] result into a bounded queue: when full, the
    /// oldest item is evicted (latest-wins) and the dropped counter grows.
    /// The session is only ever polled — the watcher can never block it.
    /// [`Watcher::inject`] sends input through the same owned session.
    pub struct Watcher {
        session: Arc<Session>,
        inner: Arc<WatcherInner>,
        capacity: usize,
        thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    }

    // Manual: `Session` offers no `Debug`.
    impl std::fmt::Debug for Watcher {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Watcher").finish_non_exhaustive()
        }
    }

    impl Watcher {
        /// Subscribe to `session`. `capacity` bounds the queue (0 becomes 1);
        /// `poll` is the revision-poll cadence.
        ///
        /// Infallible by design: if the poll thread cannot spawn, the
        /// watcher holds no thread, so [`Watcher::next_timeout`] and
        /// [`Watcher::try_next`] yield nothing and [`Watcher::stop`] is a
        /// no-op. Spawn failure is a resource-exhaustion signal, not data.
        pub fn subscribe(session: Arc<Session>, capacity: usize, poll: Duration) -> Self {
            let inner = Arc::new(WatcherInner {
                queue: Mutex::new(VecDeque::new()),
                changed: Condvar::new(),
                dropped: AtomicU64::new(0),
                stop: AtomicBool::new(false),
            });
            let capacity = capacity.max(1);
            let worker_session = Arc::clone(&session);
            let worker_inner = Arc::clone(&inner);
            let thread = std::thread::Builder::new()
                .name("tuiscotti-observe-watcher".to_string())
                .spawn(move || watch_loop(&worker_session, &worker_inner, capacity, poll))
                .ok();
            Self {
                session,
                inner,
                capacity,
                thread: Mutex::new(thread),
            }
        }

        /// Queue bound.
        #[must_use]
        pub fn capacity(&self) -> usize {
            self.capacity
        }

        /// Total observations evicted so far (latest-wins lag signal).
        #[must_use]
        pub fn dropped(&self) -> u64 {
            self.inner.dropped.load(Ordering::SeqCst)
        }

        /// Take the oldest queued observation, waiting up to `timeout`.
        /// Returns `None` on timeout (or after [`Watcher::stop`]).
        pub fn next_timeout(&self, timeout: Duration) -> Option<Watched> {
            let deadline = Instant::now() + timeout;
            let mut q = self
                .inner
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            while q.is_empty() {
                if self.inner.stop.load(Ordering::SeqCst) {
                    return None;
                }
                let now = Instant::now();
                if now >= deadline {
                    return None;
                }
                let (guard, _) = self
                    .inner
                    .changed
                    .wait_timeout(q, deadline - now)
                    .unwrap_or_else(PoisonError::into_inner);
                q = guard;
            }
            q.pop_front().map(|observation| Watched {
                observation,
                dropped_before: self.inner.dropped.load(Ordering::SeqCst),
            })
        }

        /// Take the oldest queued observation without waiting.
        pub fn try_next(&self) -> Option<Watched> {
            let mut q = self
                .inner
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            q.pop_front().map(|observation| Watched {
                observation,
                dropped_before: self.inner.dropped.load(Ordering::SeqCst),
            })
        }

        /// Inject raw input bytes through the watched session.
        ///
        /// # Errors
        ///
        /// Returns [`TuiError`] when the session rejects or cannot take input.
        pub fn inject(&self, bytes: &[u8]) -> Result<(), TuiError> {
            self.session.send_bytes(bytes)
        }

        /// Inject literal text through the watched session.
        ///
        /// # Errors
        ///
        /// Returns [`TuiError`] when the session rejects or cannot take input.
        pub fn inject_text(&self, text: &str) -> Result<(), TuiError> {
            self.session.send_text(text)
        }

        /// Stop the poll thread and drop queued items. Idempotent.
        pub fn stop(&self) {
            self.inner.stop.store(true, Ordering::SeqCst);
            self.inner.changed.notify_all();
            if let Ok(mut guard) = self.thread.lock()
                && let Some(h) = guard.take()
            {
                // A panicked poll thread still ends the subscription:
                // stop stays idempotent and the queue is dropped either
                // way, so the join outcome changes nothing.
                if h.join().is_err() {
                    // Poll thread panicked; already stopped.
                }
            }
        }
    }

    impl Drop for Watcher {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn watch_loop(session: &Session, inner: &WatcherInner, capacity: usize, poll: Duration) {
        let mut pushed = session.revision();
        // Publish the current frame immediately so a quiet session still
        // yields one observation.
        if let Ok(obs) = session.observe_now() {
            pushed = pushed.max(obs.revision);
            push_watched(inner, obs, capacity);
        }
        loop {
            if inner.stop.load(Ordering::SeqCst) {
                return;
            }
            if session.revision() > pushed {
                match session.observe_now() {
                    Ok(obs) => {
                        if obs.revision > pushed {
                            pushed = obs.revision;
                            push_watched(inner, obs, capacity);
                        } else {
                            pushed = pushed.max(session.revision());
                        }
                    }
                    Err(TuiError::Closed(_)) => return,
                    Err(_) => std::thread::sleep(poll),
                }
            } else {
                std::thread::sleep(poll);
            }
        }
    }

    fn push_watched(inner: &WatcherInner, obs: Observation, capacity: usize) {
        let mut q = inner.queue.lock().unwrap_or_else(PoisonError::into_inner);
        while q.len() >= capacity {
            q.pop_front();
            inner.dropped.fetch_add(1, Ordering::SeqCst);
        }
        q.push_back(obs);
        drop(q);
        inner.changed.notify_one();
    }
}

#[cfg(feature = "pty")]
pub use pty_paths::{Watched, Watcher};
