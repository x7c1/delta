//! Inputs for building one session's push-based content accumulator.

use delta_model::{SessionId, ThreadId};

#[cfg(doc)]
use super::AgentAdapter;
#[cfg(doc)]
use crate::agent::AgentContentSource;

/// Inputs for building one session's push-based content accumulator
/// ([`AgentAdapter::content_source`]).
///
/// Everything here is a **per-session** fact: known once the session is bound
/// and constant for its lifetime. That is why the accumulator takes them at
/// construction rather than being told them again for every turn — the
/// *per-turn* routing context travels separately, through
/// [`AgentContentSource::begin_turn`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSourceRequest {
    /// Delta's own conversation id, stamped on every message the source folds.
    pub session_id: SessionId,
    /// The session's `main` thread: where a plain turn's messages land, and
    /// what each turn's routing resets to.
    pub main_thread: ThreadId,
    /// The first `seq` to mint — the store's current `MAX(seq) + 1`, so minted
    /// ordering continues past whatever is already persisted (`0` for a fresh
    /// session, the persisted message count on a resume).
    pub seed_seq: i64,
    /// The directory the session's agent runs in: the launch directory Delta
    /// resolved at spawn and recorded on the session row (the git worktree path
    /// when the session was started with one).
    pub cwd: String,
    /// The git branch checked out in [`Self::cwd`], observed when the session was
    /// bound. `None` when that directory is not a git working tree, when HEAD is
    /// detached there, or when git could not be consulted.
    ///
    /// Observed rather than copied from the session row's `branch_at_launch`,
    /// which is only filled on the worktree spawn path: a session started in a
    /// plain directory would otherwise report no branch despite obviously having
    /// one. It is also the fresher fact — `branch_at_launch` is a spawn-time
    /// snapshot that is deliberately never updated, while this describes the
    /// working tree the messages are actually produced against, re-observed on
    /// every bind (including a resume after a restart).
    pub git_branch: Option<String>,
}
