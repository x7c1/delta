//! Capturing `warn`-level tracing output so a test can assert that a failure
//! the code deliberately carries on past is still logged.

use std::io::Write;
use std::sync::{Arc, Mutex};

use tracing::subscriber::DefaultGuard;
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
pub(crate) fn capture_warnings() -> (LogCapture, DefaultGuard) {
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
