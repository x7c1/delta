use std::path::Path;
use std::sync::Mutex;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use super::{app_log_dir, resolve_filter, FileWriter, FilterSource, LogGuard, FILTER_FILE_NAME};

/// Install the global `tracing` subscriber for the app under `identifier`.
pub fn init(identifier: &str) -> LogGuard {
    let filter_file =
        Path::new(&delta_server::config::data_dir_from_env_for(identifier)).join(FILTER_FILE_NAME);
    let resolved = resolve_filter(
        std::env::var("RUST_LOG").ok().as_deref(),
        &filter_file,
        std::fs::read_to_string(&filter_file),
    );
    let file = FileWriter::open(app_log_dir(identifier).as_deref());

    let (file_layer, worker, dir, file_unavailable) = match file {
        FileWriter::Writing { dir, writer, guard } => (
            Some(fmt::layer().with_writer(writer).with_ansi(false)),
            Some(guard),
            Some(dir),
            None,
        ),
        FileWriter::StdoutOnly(unavailable) => (None, None, None, Some(unavailable)),
    };
    tracing_subscriber::registry()
        // Validated by `resolve_filter`.
        .with(EnvFilter::new(&resolved.directives))
        .with(fmt::layer())
        .with(file_layer)
        .init();

    if let Some(fallback) = &resolved.fallback {
        tracing::warn!("{fallback}");
    }
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
