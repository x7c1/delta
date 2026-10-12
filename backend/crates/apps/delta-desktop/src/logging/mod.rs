//! The desktop app's log: to stdout, and to a file in the app log directory,
//! at a level that can be changed without an environment variable.
//!
//! An app started from the launcher, the Dock or Finder is given no
//! environment variable and, on macOS, nothing keeps its stdout, so:
//!
//! - **Where.** Every line goes to stdout, as the `delta-server` binary's do
//!   (the Linux desktop session's journal keeps them, and `make desktop-dev`
//!   shows them), and to `delta-desktop.<date>.log` in the app log directory
//!   ([`app_log_dir()`]). A new file is started each day and only the last
//!   [`KEPT_FILES`] are kept. The file is written by a background worker, so a
//!   request never waits on the disk. A directory that cannot be created or
//!   written is logged at `warn`, and the app carries on with stdout only.
//! - **What.** `RUST_LOG` when set; otherwise the `EnvFilter` directives in
//!   [`FILTER_FILE_NAME`] in the data directory (`DELTA_DATA_DIR`, or
//!   `<platform data dir>/<identifier>`); otherwise `info`
//!   ([`resolve_filter()`]). It is read once, at launch.
//!
//! The worker holds lines not yet written until [`LogGuard::flush`] writes them
//! out. `main` calls it on every way the app ends: in its handler of
//! `RunEvent::Exit`, which every exit after the app is built goes through (the
//! list is in `window_size`'s docs), and before each of the two
//! `process::exit` calls that come earlier.

mod app_log_dir;
use app_log_dir::app_log_dir;

mod file_writer;
use file_writer::FileWriter;

mod filter_fallback;
use filter_fallback::FilterFallback;

mod filter_source;
use filter_source::FilterSource;

mod init;
pub use init::init;

mod log_file_unavailable;
use log_file_unavailable::LogFileUnavailable;

mod log_guard;
pub use log_guard::LogGuard;

mod resolve_filter;
use resolve_filter::resolve_filter;

mod resolved_filter;
use resolved_filter::ResolvedFilter;

mod warn_if_the_resolver_disagrees;
pub use warn_if_the_resolver_disagrees::warn_if_the_resolver_disagrees;

/// The file in the data directory that holds the app's filter directives.
const FILTER_FILE_NAME: &str = "log-filter";

/// The directives used when neither `RUST_LOG` nor the filter file names any.
const DEFAULT_DIRECTIVES: &str = "info";

/// How many daily files are kept; the oldest is deleted as a new day's opens.
const KEPT_FILES: usize = 7;
