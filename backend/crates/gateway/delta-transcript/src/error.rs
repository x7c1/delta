//! Crate-local error type for the transcript gateway.

use std::path::{Path, PathBuf};

use delta_usecase::{TranscriptError, TranscriptRefusal};
use thiserror::Error;

/// Errors raised by [`crate::JsonlTranscript`].
#[derive(Debug, Error)]
pub enum Error {
    /// A filesystem operation failed. `path` is what it operated on — the
    /// transcript file, or the transcript root being enumerated — so the
    /// message says which file or directory failed; the failure itself is the
    /// `source`.
    #[error("transcript io error at {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The transcript path names something other than a regular file — a FIFO,
    /// a device, a directory. Refused rather than read: reading a FIFO would
    /// block until a writer appears, and a device may never end.
    #[error("transcript at {} is not a regular file", path.display())]
    NotRegularFile { path: PathBuf },

    /// The transcript is larger than the reader accepts (see
    /// [`crate::MAX_TRANSCRIPT_BYTES`]). Refused rather than read as empty, so
    /// the caller reports it instead of the session silently going quiet.
    #[error(
        "transcript at {} is {size} bytes, over the {cap}-byte limit",
        path.display()
    )]
    TooLarge { path: PathBuf, size: u64, cap: u64 },

    /// The blocking task reading the transcript at `path` ended without a
    /// result: it panicked, or the runtime cancelled it while shutting down.
    #[error("transcript read at {} was aborted", path.display())]
    ReadAborted {
        path: PathBuf,
        #[source]
        source: tokio::task::JoinError,
    },
}

impl Error {
    /// An [`Error::Io`] for a failure operating on `path`.
    pub(crate) fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Each kind maps onto its use-case counterpart, keeping the values a
/// refusal's message needs and the underlying cause as the `source`, so a
/// caller can tell a lasting refusal from a failure that may pass.
impl From<Error> for TranscriptError {
    fn from(value: Error) -> Self {
        match value {
            Error::Io { path, source } => TranscriptError::Io { path, source },
            Error::NotRegularFile { path } => TranscriptError::Refused {
                path,
                reason: TranscriptRefusal::NotRegularFile,
            },
            Error::TooLarge { path, size, cap } => TranscriptError::Refused {
                path,
                reason: TranscriptRefusal::TooLarge { size, cap },
            },
            Error::ReadAborted { path, source } => TranscriptError::Aborted {
                path,
                source: Box::new(source),
            },
        }
    }
}

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Transcript(value.into())
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use delta_usecase::with_causes;

    use super::*;

    fn converted(err: Error) -> delta_usecase::Error {
        delta_usecase::Error::from(err)
    }

    #[test]
    fn an_io_error_names_the_path_that_failed() {
        let err = Error::io(
            "/p/-work/sess-1.jsonl",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        let message = err.to_string();
        assert!(
            message.contains("/p/-work/sess-1.jsonl"),
            "the message must name the path, got {message:?}"
        );
        // The path and the cause survive the conversion the use case layer
        // logs.
        let converted = with_causes(&converted(err));
        assert!(
            converted.contains("/p/-work/sess-1.jsonl"),
            "the converted message must name the path, got {converted:?}"
        );
        assert!(
            converted.contains("permission denied"),
            "the converted message must carry the cause, got {converted:?}"
        );
    }

    #[test]
    fn an_io_error_stays_an_io_error_with_its_cause() {
        let err = converted(Error::io(
            "/p/t.jsonl",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        ));
        assert!(matches!(
            &err,
            delta_usecase::Error::Transcript(TranscriptError::Io { path, source })
                if path == Path::new("/p/t.jsonl")
                    && source.kind() == std::io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn a_file_that_is_not_regular_is_a_refusal() {
        let err = converted(Error::NotRegularFile {
            path: PathBuf::from("/p/fifo.jsonl"),
        });
        assert!(matches!(
            &err,
            delta_usecase::Error::Transcript(TranscriptError::Refused {
                path,
                reason: TranscriptRefusal::NotRegularFile,
            }) if path == Path::new("/p/fifo.jsonl")
        ));
        assert_eq!(
            err.to_string(),
            "transcript at /p/fifo.jsonl is not a regular file"
        );
    }

    #[test]
    fn a_file_over_the_limit_is_a_refusal_carrying_its_size() {
        let err = converted(Error::TooLarge {
            path: PathBuf::from("/p/big.jsonl"),
            size: 11,
            cap: 10,
        });
        assert!(matches!(
            &err,
            delta_usecase::Error::Transcript(TranscriptError::Refused {
                reason: TranscriptRefusal::TooLarge { size: 11, cap: 10 },
                ..
            })
        ));
        assert_eq!(
            err.to_string(),
            "transcript at /p/big.jsonl is 11 bytes, over the 10-byte limit"
        );
    }

    #[tokio::test]
    async fn an_aborted_read_stays_aborted_with_the_join_error() {
        let join_error = tokio::spawn(async { panic!("reader panicked") })
            .await
            .unwrap_err();
        let err = converted(Error::ReadAborted {
            path: PathBuf::from("/p/t.jsonl"),
            source: join_error,
        });
        let delta_usecase::Error::Transcript(TranscriptError::Aborted { path, source }) = &err
        else {
            panic!("expected an aborted read, got {err:?}");
        };
        assert_eq!(path, Path::new("/p/t.jsonl"));
        assert!(
            source
                .downcast_ref::<tokio::task::JoinError>()
                .is_some_and(tokio::task::JoinError::is_panic),
            "the join error is kept as the source, got {source:?}"
        );
    }
}
