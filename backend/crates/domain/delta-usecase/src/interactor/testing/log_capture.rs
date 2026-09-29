//! Capturing `warn`-level tracing output so a test can assert that a failure
//! the code deliberately carries on past is still logged.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::subscriber::{DefaultGuard, NoSubscriber};
use tracing::Dispatch;
use tracing_subscriber::fmt;

/// The buffer a [`capture_warnings`] subscriber writes into.
#[derive(Clone, Default)]
pub(crate) struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    /// Everything logged so far, as plain text (no ANSI, no timestamps).
    pub(crate) fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Install a thread-local subscriber that records `warn` and above until the
/// returned guard drops, and hand back the buffer it writes to.
///
/// The subscriber is the thread's default only (`set_default`), so it does not
/// leak across tests; the `#[tokio::test]` current-thread runtime keeps the
/// code under test on that thread. Hold the guard until the test ends.
///
/// Before installing it, [`anchor_dispatcher`] makes sure the callsite cache
/// takes every live thread-local subscriber into account; without that, a
/// capture could silently miss an event another test hit first.
pub(crate) fn capture_warnings() -> (LogCapture, DefaultGuard) {
    anchor_dispatcher();
    let capture = LogCapture::default();
    let subscriber = fmt()
        .with_writer(capture.clone())
        .with_max_level(tracing::Level::WARN)
        .without_time()
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (capture, guard)
}

/// Keep one extra dispatcher registered for the life of the test process, so
/// that `tracing` always has more than one dispatcher on record.
///
/// `tracing-core` caches each callsite's interest the first time the callsite
/// is hit. If only one dispatcher is registered when that happens, it asks
/// just the *current thread's* default subscriber. Say a capture is the only
/// dispatcher, and another test thread with no subscriber is the first to hit
/// a callsite (the same `warn!` from a test that does not capture). The
/// callsite is then cached as `never`, and the capture's own thread drops that
/// event for the rest of the run. That test passes when run alone and fails
/// only when tests run in parallel. With this no-op dispatcher always alive, a
/// capture is never the only one, so the cache asks every live dispatcher and
/// the capture's thread still receives the event.
fn anchor_dispatcher() {
    static ANCHOR: OnceLock<Dispatch> = OnceLock::new();
    ANCHOR.get_or_init(|| Dispatch::new(NoSubscriber::default()));
}

#[cfg(test)]
mod tests {
    use super::capture_warnings;

    /// One `warn!` callsite used by this test alone, so nothing else in the run
    /// can register it first.
    fn warn_from_a_callsite_of_its_own() {
        tracing::warn!("reaches the capture");
    }

    #[test]
    fn a_capture_still_sees_a_callsite_another_thread_hit_first() {
        let (logs, _guard) = capture_warnings();
        // A thread with no subscriber of its own hits the callsite first, so
        // its interest gets cached while the capture is alive.
        std::thread::spawn(warn_from_a_callsite_of_its_own)
            .join()
            .unwrap();

        warn_from_a_callsite_of_its_own();

        let logged = logs.text();
        assert!(
            logged.contains("reaches the capture"),
            "the capture's thread still gets the event: {logged:?}"
        );
    }
}
