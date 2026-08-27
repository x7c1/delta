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

    /// A send targeted a session whose launch has not bound yet: the row exists
    /// (it is listed as `spawning` from the moment its first send was accepted)
    /// but no pane is mapped to it, and its transcript does not exist yet — so
    /// the resume path would launch a second agent against nothing. The send is
    /// refused instead; the composer is disabled on a starting session, so this
    /// only fires against a stale client. Surfaced as `409`.
    #[error("session is still starting: {0}")]
    SessionSpawning(String),

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
    /// Surfaced as `409` so the browser switches to guidance chosen by the
    /// provider's `has_terminal` capability.
    #[error("permission request {0} is not awaiting a decision")]
    PermissionNotPending(i64),

    /// A permission decision the session's provider has no meaning for — today
    /// a session-scoped allow posted against a provider that does not declare
    /// [`SessionScopedAllowCapability::Supported`](crate::SessionScopedAllowCapability::Supported).
    /// Surfaced as `400` rather than `409`: nothing is wrong with current
    /// state — the request stays pending and answerable with a plain allow or
    /// deny, and no decision reaches the provider — it is the body's decision
    /// value this provider cannot express. Refused rather than degraded into a
    /// plain allow, which would keep prompting a user who asked to stop being
    /// prompted with nothing on screen saying why.
    #[error(
        "permission request {0} cannot be answered with this decision: \
         the session's provider does not support it"
    )]
    PermissionDecisionUnsupported(i64),

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
    /// unknown, the send is already terminal (matched a transcript line, or
    /// was already cancelled), or its echo has already arrived (the turn
    /// carries it in flight). Surfaced as `409` so the browser drops the
    /// cancel control and reconciles its pending strip from the next refetch.
    #[error("send {0} is not cancellable")]
    SendNotCancellable(i64),

    /// A release arrived for a send that is not awaiting one: the id is
    /// unknown, the row was never held for one (neither the boot-time
    /// reconcile nor the echo-deadline park marked it), it was already
    /// released, or it has since been cancelled. Surfaced as `409` so the
    /// browser drops the Send control and reconciles its pending strip from
    /// the next refetch.
    #[error("send {0} is not awaiting a release")]
    SendNotReleasable(i64),

    /// A clone root was registered twice with the same path. Surfaced as `409`
    /// so the Settings dialog can show an inline "already registered" hint
    /// without a generic failure toast.
    #[error("clone root already registered: {0}")]
    CloneRootDuplicate(String),

    /// A clone was requested into a directory that is not a registered clone
    /// root. Delta only ever writes clones into roots the user registered, so an
    /// unregistered destination is refused before any `gh` process starts.
    /// Surfaced as `400` with a stable code so the browser can say *why* rather
    /// than showing a generic failure.
    #[error("not a registered clone root: {0}")]
    CloneRootNotRegistered(String),

    /// The destination a clone would land on (`<clone_root>/<repo_name>`)
    /// already exists. Delta never clones onto an existing path — there is no
    /// fallback naming — so the request is refused with no job started.
    /// Surfaced as `409` with a stable code so the row can show an inline
    /// "already there" message.
    #[error("clone destination already exists: {0}")]
    CloneDestinationExists(String),

    /// A clone request named an owner or repository that cannot be part of a
    /// path: blank, or carrying a path separator, a `..` segment, or a NUL.
    /// Refused as `400` — the destination is built by joining the name onto the
    /// clone root, so accepting one would let a request write outside the root.
    #[error("invalid repository reference: {0}")]
    InvalidRepositoryRef(String),

    /// A prompt template was submitted with a blank `label` or a blank `text`
    /// (blank meaning empty once surrounding whitespace is trimmed). Surfaced as
    /// `400`: an unnamed template is unpickable and an empty one inserts
    /// nothing, so neither is worth storing. Only the *check* trims — the stored
    /// text keeps its own leading and trailing whitespace, since a template may
    /// deliberately end with a newline.
    #[error("invalid prompt template: {0}")]
    InvalidPromptTemplate(String),

    /// A driver (tmux) failure.
    #[error("tmux driver error: {0}")]
    Tmux(String),

    /// An agent adapter / transport failure (e.g. the app-server connection
    /// dropped, or a provider RPC errored). Reported by a gateway adapter as it
    /// crosses the [`crate::AgentAdapter`] trait boundary. Surfaced as `500`.
    #[error("agent error: {0}")]
    Agent(String),

    /// A selected launch option cannot be applied to the session being started:
    /// it names a field the provider's adapter reserves for Delta, or the same
    /// field twice. Reported by a gateway adapter as it renders the launch
    /// request for its provider. Surfaced as `400` — the request named a
    /// selection the server will not honour, and the message says which one, so
    /// the user can fix the registry entry (a silent drop or a silent override
    /// would leave them debugging an agent that ignored their setting, or worse,
    /// one running somewhere Delta did not record).
    #[error("launch option rejected: {0}")]
    LaunchOptionRejected(String),

    /// A delete was requested for a launch option Delta *ships* (a row carrying
    /// a `builtin_key`). The declared catalog is the source of truth for those
    /// rows, so they are not the user's to remove: a shipped option that does
    /// not suit is left unticked, and registering your own row is the supported
    /// way to differ.
    ///
    /// Surfaced as `409`, not `400`: the request value is a perfectly good id
    /// and the same call against a user row is honoured — it is the *target's*
    /// current state that forbids the operation, the line
    /// [`Self::PermissionNotPending`] already draws. `PATCH` on the same row
    /// still succeeds; ticking `default_enabled` on a shipped option is the
    /// point of shipping it.
    #[error("launch option {0} is built in and cannot be deleted")]
    LaunchOptionIsBuiltin(i64),

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

    /// A freshly-accepted session's launch preparation (worktree build, trust
    /// seed, settings write, agent launch) outran its deadline — a `git fetch`
    /// hanging on an unreachable remote or a credential prompt, say. Never
    /// returned to a REST caller: the send was accepted long before this could
    /// happen, so it reaches the browser as the `reason` of a
    /// [`SessionEvent::SpawnFailed`](crate::ports::SessionEvent::SpawnFailed).
    #[error("launch preparation timed out: {0}")]
    LaunchPreparationTimedOut(String),

    /// A freshly-accepted session's worktree build landed on a different path
    /// than the accept phase planned, so the session row's `cwd` names a
    /// directory that does not exist.
    ///
    /// Only reachable for a `use_remote_branch` start point, whose plan is
    /// "reuse the worktree already holding the branch, else create one": a
    /// second session started from the same branch while the first is still
    /// checking out plans the default path, then finds the first session's
    /// worktree at build time. Git forbids one branch in two worktrees, so
    /// there is nothing to build at the planned path and nothing to re-point
    /// the (already persisted) `cwd` at — the launch fails instead of starting
    /// the agent in a directory that is not there. A retry re-plans and, the
    /// worktree now existing, reuses it.
    ///
    /// Never returned to a REST caller: like
    /// [`Self::LaunchPreparationTimedOut`] it happens long after the send was
    /// accepted, so it reaches the browser as the `reason` of a
    /// [`SessionEvent::SpawnFailed`](crate::ports::SessionEvent::SpawnFailed).
    #[error(
        "the worktree for branch {branch} landed on {built}, not on the planned {planned}; \
         retry the session to start it in {built}"
    )]
    WorktreeLandedElsewhere {
        /// The branch the worktree checks out.
        branch: String,
        /// The launch directory the accept phase planned (and stored as `cwd`).
        planned: String,
        /// Where the worktree holding `branch` actually is.
        built: String,
    },

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
