//! The startup warning that a filter records everything at `trace`.
//!
//! At `trace` the log records far more than the server's own decisions: what
//! is typed into a terminal and what it prints, and the payload of every frame
//! on the live channels (the WebSocket library logs frame contents at
//! `trace`). Such a log holds the user's prompts, the agents' replies and
//! whatever the terminal showed, so it must not be shared as it is. Each shell
//! calls [`warn_if_tracing_everything`] once its subscriber is installed.

use tracing::level_filters::LevelFilter;

/// Log one `warn` line when `directives` (an `EnvFilter` directive list, as
/// the subscriber was built from) enable `trace` for every target.
pub fn warn_if_tracing_everything(directives: &str) {
    if traces_everything(directives) {
        tracing::warn!(
            filter = directives,
            "the log filter enables `trace` for everything: the log records terminal \
             input and output and live-channel payloads, so do not share it as it is"
        );
    }
}

/// Whether `directives` hold a directive that names no target and sets
/// `trace` (`trace`, or `info,trace`). A directive for one target
/// (`delta_server=trace`) or one span is not everything.
fn traces_everything(directives: &str) -> bool {
    directives
        .split(',')
        .map(str::trim)
        .any(|directive| matches!(directive.parse::<LevelFilter>(), Ok(LevelFilter::TRACE)))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    #[test]
    fn only_a_targetless_trace_directive_traces_everything() {
        for directives in ["trace", "TRACE", "info,trace", " info , trace "] {
            assert!(traces_everything(directives), "{directives}");
        }
        for directives in [
            "info",
            "debug",
            "info,delta_server=trace",
            "tungstenite=trace",
            "[request]=trace",
            "",
        ] {
            assert!(!traces_everything(directives), "{directives}");
        }
    }

    /// Where the subscriber under test writes.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Captured;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// What [`warn_if_tracing_everything`] logs for `directives`.
    fn logged_for(directives: &str) -> String {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(captured.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || warn_if_tracing_everything(directives));
        let bytes = captured.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn the_warning_is_logged_only_when_everything_is_traced() {
        let logged = logged_for("info,trace");
        assert_eq!(logged.matches("WARN").count(), 1, "{logged}");
        assert!(
            logged.contains("enables `trace` for everything"),
            "{logged}"
        );

        for directives in ["info", "info,delta_server::http=debug", "tungstenite=trace"] {
            let logged = logged_for(directives);
            assert!(logged.is_empty(), "{directives}: {logged}");
        }
    }
}
