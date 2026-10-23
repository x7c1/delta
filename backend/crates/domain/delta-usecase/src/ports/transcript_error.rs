//! Why a transcript could not be read.

use std::fmt;
use std::path::PathBuf;

/// Why a [`Transcript`](super::Transcript) read failed.
///
/// The kinds a caller acts on differently are kept apart: a
/// [`Self::Refused`] transcript is refused again on every read until the file
/// itself changes, while the others may pass on the next read. Each variant's
/// text names only its own level; the error it was caused by is its `source`,
/// to be rendered once with [`crate::with_causes()`].
#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    /// The reader refuses the transcript outright, for `reason`. Nothing about
    /// the read itself went wrong, so the same file is refused again on the
    /// next read: only a change to the file ends it.
    #[error("transcript at {} {reason}", path.display())]
    Refused {
        path: PathBuf,
        reason: TranscriptRefusal,
    },

    /// A filesystem operation failed. `path` is what it operated on — the
    /// transcript file, or the transcript root being enumerated.
    #[error("transcript io error at {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The task reading the transcript at `path` ended without a result: it
    /// panicked, or the runtime cancelled it while shutting down.
    #[error("transcript read at {} was aborted", path.display())]
    Aborted {
        path: PathBuf,
        /// The task's own error, boxed, as this crate does not name the
        /// runtime's join error type (a caller may downcast it).
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Line `line` (1-based) of the transcript at `path` could not be parsed as
/// JSON; the parser's own error is the `source`, to be rendered with
/// [`crate::with_causes()`].
///
/// Not a [`TranscriptError`]: an unparsable line never fails a read. The read
/// skips it and hands it back in
/// [`TranscriptRead::unparsable_lines`](super::TranscriptRead::unparsable_lines)
/// for the caller to report.
#[derive(Debug, thiserror::Error)]
#[error("transcript parse error at {} line {line}", path.display())]
pub struct UnparsableTranscriptLine {
    pub path: PathBuf,
    pub line: usize,
    #[source]
    pub source: serde_json::Error,
}

/// Why the reader refuses a transcript ([`TranscriptError::Refused`]).
///
/// A plain value rather than an error: it carries what the message needs and
/// lets a caller tell one refusal from another.
#[derive(Debug, Clone, Copy)]
pub enum TranscriptRefusal {
    /// The path names something other than a regular file — a FIFO, a device,
    /// a directory. Reading a FIFO would block until a writer appears, and a
    /// device may never end.
    NotRegularFile,
    /// The file is `size` bytes, over the `cap` bytes the reader accepts.
    TooLarge { size: u64, cap: u64 },
}

impl fmt::Display for TranscriptRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegularFile => f.write_str("is not a regular file"),
            Self::TooLarge { size, cap } => {
                write!(f, "is {size} bytes, over the {cap}-byte limit")
            }
        }
    }
}
