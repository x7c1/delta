//! Where a send should land.

use delta_model::{MessageUuid, ThreadId};

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
    },
}
