use std::fmt;
use std::path::PathBuf;

use tracing_appender::rolling::InitError;

/// Why the log goes to stdout only. Its `Display` is the warning logged once
/// logging is up.
#[derive(Debug)]
pub enum LogFileUnavailable {
    /// The app log directory could not be named: there is no home directory to
    /// resolve it from.
    NoHomeDirectory,
    /// The log file could not be opened in `dir`: the directory could not be
    /// created or written.
    Unwritable { dir: PathBuf, cause: InitError },
}

impl fmt::Display for LogFileUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHomeDirectory => f.write_str(
                "could not name the app log directory (no home directory); logging to stdout only",
            ),
            Self::Unwritable { dir, cause } => write!(
                f,
                "could not open a log file in {} ({cause}); logging to stdout only",
                dir.display()
            ),
        }
    }
}
