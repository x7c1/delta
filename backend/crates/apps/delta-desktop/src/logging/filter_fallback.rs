use std::fmt;
use std::io;
use std::path::PathBuf;

use tracing_subscriber::filter::ParseError;

use super::DEFAULT_DIRECTIVES;

/// Why the directives asked for were not used and the filter fell back to
/// [`DEFAULT_DIRECTIVES`]. Its `Display` is the warning logged once logging is
/// up.
#[derive(Debug)]
pub enum FilterFallback {
    /// `RUST_LOG` held `directives`, which do not parse.
    InvalidRustLog {
        directives: String,
        cause: ParseError,
    },
    /// The filter file at `file` exists but could not be read.
    UnreadableFile { file: PathBuf, cause: io::Error },
    /// The filter file at `file` holds `directives`, which do not parse.
    InvalidFile {
        file: PathBuf,
        directives: String,
        cause: ParseError,
    },
}

impl fmt::Display for FilterFallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRustLog { directives, cause } => write!(
                f,
                "RUST_LOG={directives:?} is not a valid log filter ({cause}); logging at {DEFAULT_DIRECTIVES}"
            ),
            Self::UnreadableFile { file, cause } => write!(
                f,
                "could not read the log filter file {} ({cause}); logging at {DEFAULT_DIRECTIVES}",
                file.display()
            ),
            Self::InvalidFile {
                file,
                directives,
                cause,
            } => write!(
                f,
                "the log filter file {} holds an invalid filter {directives:?} ({cause}); logging at {DEFAULT_DIRECTIVES}",
                file.display()
            ),
        }
    }
}
