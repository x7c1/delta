//! Use-case-level errors.
//!
//! Each capability trait reports failures through this single error type. The
//! gateway crates define their own errors and convert into [`Error`] when they
//! cross the trait boundary, keeping the dependency direction intact.

use thiserror::Error;

/// Errors raised while executing a use case.
#[derive(Debug, Error)]
pub enum Error {
    /// The session has not been registered yet (no `UserPromptSubmit` seen).
    #[error("no session registered")]
    NoSession,

    /// A referenced thread does not exist.
    #[error("thread not found: {0}")]
    ThreadNotFound(i64),

    /// A driver (tmux) failure.
    #[error("tmux driver error: {0}")]
    Tmux(String),

    /// A transcript read/parse failure.
    #[error("transcript error: {0}")]
    Transcript(String),

    /// A persistence failure.
    #[error("store error: {0}")]
    Store(String),

    /// An invalid domain value surfaced from the model layer.
    #[error("model error: {0}")]
    Model(#[from] delta_model::Error),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
