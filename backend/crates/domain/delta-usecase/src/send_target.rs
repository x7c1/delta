//! Where a send should land.

use delta_model::{MessageUuid, ThreadId};

use crate::agent::AgentProvider;
use crate::ports::WorktreeStartPoint;

/// An opt-in request to start a fresh session inside a git worktree.
///
/// When a `NewSession` carries a `WorktreeSpec` and a selected working
/// directory that is a git repository, Delta creates a per-session worktree and
/// launches the session there instead of in the selected directory itself. The
/// only knob is where the worktree's branch starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSpec {
    /// Where the new worktree's branch should start from (current `HEAD` or a
    /// fetched remote branch).
    pub start_point: WorktreeStartPoint,
}

/// The target a send is directed at.
///
/// A send no longer implies "the single session": the caller states whether the
/// message continues an existing conversation or starts a fresh one. The session
/// is then determined by this target — derived from the thread for an existing
/// send, or created for a new one — never by a global "current" session.
#[derive(Debug, Clone)]
pub enum SendTarget {
    /// Continue an existing session by sending into one of its threads.
    ///
    /// The session is derived from the thread (threads belong to a session), so
    /// the caller need not name it. For a plain send `thread_id` is the target
    /// thread; for a branch send it is the parent thread the new child hangs off
    /// and `branch_from` is the message the branch roots at.
    Thread {
        thread_id: ThreadId,
        /// When set, this is the first message of a new branch: an unnamed child
        /// thread is created off this message and the send is attributed to it.
        branch_from: Option<MessageUuid>,
    },
    /// Start a fresh session, landing the first message on its `main` thread.
    ///
    /// No thread (and no session) exists yet: a session is spawned with the text
    /// held as its first prompt, and the conversational id is learned when
    /// the first `UserPromptSubmit` hook binds the spawn. Any `locator_quote` is
    /// ignored — a brand-new session has no earlier passage to anchor — so the
    /// held first prompt carries no quote.
    NewSession {
        /// The working directory the session should launch in. When `Some`, it
        /// is a user-selected path validated (and canonicalized) before launch;
        /// when `None`, the session uses its default per-spawn `<base>/<token>`
        /// directory.
        workdir: Option<String>,
        /// The user-selected subset of registered launch options to apply to
        /// the spawned `claude`, in selection order. Each id is resolved to its
        /// registered `(name, value?)` flag record at spawn and pushed onto the
        /// launch argv. Empty when the user selected none.
        launch_option_ids: Vec<i64>,
        /// An opt-in request to start the session inside a git worktree of the
        /// selected `workdir`. When `Some`, the selected directory must be a git
        /// repository: Delta creates a per-session worktree and launches there
        /// instead of in the directory itself. When `None`, the session starts
        /// directly in `workdir` (the unchanged behavior).
        worktree: Option<WorktreeSpec>,
        /// The AI-agent backend to launch this session on. Defaults to
        /// [`AgentProvider::Claude`] at the wire boundary, so an omitted
        /// provider reproduces the historical (Claude, tmux + hooks) behavior
        /// byte-for-byte. A [`AgentProvider::Codex`] session is launched
        /// terminal-less over `codex app-server` instead of a tmux pane.
        provider: AgentProvider,
    },
}
