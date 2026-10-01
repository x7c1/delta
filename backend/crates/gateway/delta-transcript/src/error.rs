//! Crate-local error type for the transcript gateway.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors raised by [`crate::JsonlTranscript`].
#[derive(Debug, Error)]
pub enum Error {
    /// A filesystem operation failed. `path` is what it operated on — the
    /// transcript file, or the transcript root being enumerated — so the
    /// message says which file or directory failed.
    #[error("transcript io error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A line could not be parsed as JSON.
    #[error("transcript parse error: {0}")]
    Parse(#[from] serde_json::Error),
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

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Transcript(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // The path survives the conversion the use case layer logs.
        let converted = delta_usecase::Error::from(err).to_string();
        assert!(
            converted.contains("/p/-work/sess-1.jsonl"),
            "the converted message must name the path, got {converted:?}"
        );
    }
}
