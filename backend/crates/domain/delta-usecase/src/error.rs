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

    /// A cancel arrived for a send that can no longer be cancelled: the id is
    /// unknown, or the send has already left the `queued` state (it was
    /// dispatched into the pane, matched a transcript line, or was already
    /// cancelled). Surfaced as `409` so the browser drops the cancel control
    /// and reconciles its pending strip from the next refetch.
    #[error("send {0} is not cancellable")]
    SendNotCancellable(i64),

    /// A repository scan root was registered twice with the same path. Surfaced
    /// as `409` so the Settings dialog can show an inline "already registered"
    /// hint without a generic failure toast.
    #[error("scan root already registered: {0}")]
    RepositoryScanRootDuplicate(String),

    /// A driver (tmux) failure.
    #[error("tmux driver error: {0}")]
    Tmux(String),

    /// A worktree was requested for a fresh session, but the selected working
    /// directory is not inside a git repository. The caller named a directory
    /// that cannot host a worktree, so this is surfaced as `400`.
    #[error("not a git repository: {0}")]
    WorktreeNotAGitRepo(String),

    /// A worktree was requested for a fresh session, but no working directory
    /// was selected to root it in. A worktree needs a git repository to branch
    /// off, so this request shape is rejected as `400`.
    #[error("a worktree requires a selected working directory")]
    WorktreeRequiresWorkdir,

    /// A git operation (detection or worktree creation) failed. Surfaced as a
    /// `500`: the request was well-formed, but the underlying `git` invocation
    /// errored.
    #[error("git error: {0}")]
    Git(String),

    /// A `gh` CLI invocation failed despite the gateway reporting gh as
    /// authenticated. Surfaced as `500`. Missing/unauthenticated gh is
    /// NOT routed here — it is reported via the use case's
    /// `gh_available: false` flag so the PR tab degrades gracefully.
    #[error("gh error: {0}")]
    Gh(String),

    /// The `open cwd` request named a path the server does not recognise as a
    /// working directory of any known session/message — the allowlist reject.
    /// Surfaced as `400` with a stable code so the browser can distinguish it
    /// from a generic failure; the click site never sends a path the server
    /// hasn't shown it, so this only fires against a hand-crafted request.
    #[error("path is not in the known-cwd allowlist: {0}")]
    OpenCwdPathNotAllowed(String),

    /// The `open cwd` request named a handler id that is not registered.
    /// Surfaced as `400`: the initial impl only exposes one handler
    /// (`vscode`), so anything else is a client-side bug rather than a server
    /// misconfiguration.
    #[error("unknown open-cwd handler: {0}")]
    OpenCwdUnknownHandler(String),

    /// The external-tool command (e.g. `code`) is not on `PATH`. Surfaced as
    /// `500` with a stable code so the browser can show a specific "VS Code
    /// is not installed" message instead of a generic failure — the user has
    /// an actionable fix (install the shell `code` command).
    #[error("external tool command not found on PATH: {0}")]
    ExternalOpenerCommandNotFound(String),

    /// Spawning the external-tool subprocess failed for a reason other than
    /// missing binary (fork failure, permission denied, etc.). Surfaced as
    /// `500`.
    #[error("external tool spawn failed: {0}")]
    ExternalOpenerSpawnFailed(String),

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
