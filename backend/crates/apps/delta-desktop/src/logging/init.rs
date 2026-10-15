use std::path::Path;
use std::sync::Mutex;

use tracing::Subscriber;
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, EnvFilter};

use super::{app_log_dir, resolve_filter, FileWriter, FilterSource, LogGuard, FILTER_FILE_NAME};

/// Install the global `tracing` subscriber for the app under `identifier`,
/// which also receives the records of dependencies that log through the `log`
/// crate ([`delta_server::serve::install_tracing`]).
pub fn init(identifier: &str) -> LogGuard {
    let filter_file =
        Path::new(&delta_server::config::data_dir_from_env_for(identifier)).join(FILTER_FILE_NAME);
    let resolved = resolve_filter(
        std::env::var("RUST_LOG").ok().as_deref(),
        &filter_file,
        std::fs::read_to_string(&filter_file),
    );
    let file = FileWriter::open(app_log_dir(identifier).as_deref());

    let (file_writer, worker, dir, file_unavailable) = match file {
        FileWriter::Writing { dir, writer, guard } => (Some(writer), Some(guard), Some(dir), None),
        FileWriter::StdoutOnly(unavailable) => (None, None, None, Some(unavailable)),
    };
    delta_server::serve::install_tracing(subscriber(&resolved.directives, file_writer));

    if let Some(fallback) = &resolved.fallback {
        tracing::warn!("{fallback}");
    }
    delta_server::warn_if_tracing_everything(&resolved.directives);
    if let Some(unavailable) = &file_unavailable {
        tracing::warn!("{unavailable}");
    }
    let filter_source = match &resolved.source {
        FilterSource::Env => "RUST_LOG".to_owned(),
        FilterSource::File(path) => path.display().to_string(),
        FilterSource::Default => "default".to_owned(),
    };
    tracing::info!(
        filter = %resolved.directives,
        filter_source,
        log_dir = %dir.as_deref().map_or_else(|| "-".to_owned(), |dir| dir.display().to_string()),
        "delta-desktop logging"
    );
    LogGuard {
        worker: Mutex::new(worker),
        dir,
    }
}

/// The app's subscriber: every line to stdout and, when given, without
/// colours to `file`, filtered by `directives` (validated by
/// [`resolve_filter()`]).
fn subscriber(directives: &str, file: Option<NonBlocking>) -> impl Subscriber + Send + Sync {
    tracing_subscriber::registry()
        .with(EnvFilter::new(directives))
        .with(fmt::layer())
        .with(file.map(|writer| fmt::layer().with_writer(writer).with_ansi(false)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_log_record_reaches_the_log_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("logs");
        let FileWriter::Writing { writer, guard, .. } = FileWriter::open(Some(&dir)) else {
            panic!("the log file should open in a writable directory");
        };
        // The only test in this binary that installs the global subscriber.
        // Its filter admits only this test's target, so the other tests'
        // events stay out of the file.
        delta_server::serve::install_tracing(subscriber("delta_log_bridge=info", Some(writer)));

        log::warn!(target: "delta_log_bridge", "could not read a system certificate");
        log::debug!(target: "delta_log_bridge", "below the filter");
        // Flushes the worker, as the app does on exit.
        drop(guard);

        let files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1, "{files:?}");
        let logged = std::fs::read_to_string(&files[0]).unwrap();
        assert!(
            logged.contains("WARN delta_log_bridge: could not read a system certificate"),
            "{logged:?}"
        );
        assert!(!logged.contains("below the filter"), "{logged:?}");
    }
}
