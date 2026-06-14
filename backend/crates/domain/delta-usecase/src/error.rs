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

    /// A referenced session does not exist in the store.
    #[error("session not found: {0}")]
    SessionNotFound(String),

    /// A referenced thread does not exist.
    #[error("thread not found: {0}")]
    ThreadNotFound(i64),

    /// A closed session cannot be resumed because its local transcript file is
    /// gone, so `claude --resume <id>` would have nothing to replay. The session
    /// is left closed rather than spawning a doomed pane.
    #[error("session cannot be resumed (transcript missing): {0}")]
    ResumeUnavailable(String),

    /// A user-selected working directory is not a usable directory: it does not
    /// exist, is not a directory, or could not be resolved. Surfaced as `400`.
    #[error("invalid working directory: {0}")]
    InvalidWorkdir(String),

    /// A directory could not be read because the process lacks permission.
    /// Surfaced as `403`.
    #[error("permission denied: {0}")]
    WorkdirPermission(String),

    /// A permission decision arrived for a request no browser decision can
    /// reach anymore: the id is unknown, the request was already decided, or
    /// its hook wait timed out and fell back to the interactive TUI prompt.
    /// Surfaced as `409` so the browser switches to the answer-in-the-terminal
    /// guidance.
    #[error("permission request {0} is not awaiting a decision")]
    PermissionNotPending(i64),

    /// An answer arrived for a question no longer pending: the id is unknown,
    /// it was already answered, or its turn ended. Surfaced as `409` so the
    /// browser falls back to the answer-in-the-terminal guidance.
    #[error("question request {0} is not awaiting an answer")]
    QuestionNotPending(i64),

    /// The browser's answer to a pending question could not be turned into a
    /// key sequence: a malformed selection, or a sub-case the generator refuses
    /// to drive (multi-select within a multi-question call). Surfaced as `400`.
    #[error("invalid question answer: {0}")]
    InvalidQuestionAnswer(String),

    /// A driver (tmux) failure.
    #[error("tmux driver error: {0}")]
    Tmux(String),

    /// A transcript read/parse failure.
    #[error("transcript error: {0}")]
    Transcript(String),

    /// A persistence failure.
    #[error("store error: {0}")]
    Store(String),

    /// Preparing the session working directory failed.
    #[error("workspace error: {0}")]
    Workspace(String),

    /// An internal coordination failure: a session actor went away before
    /// answering (only reachable during tear-down, or after an actor panic).
    /// Surfaced as `500`.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
