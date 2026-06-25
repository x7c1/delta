//! Guard helper for filtering hooks fired against a nested subagent's
//! transcript.
//!
//! Claude Code dispatches a nested subagent's `PreToolUse` / `PostToolUse` /
//! `PermissionRequest` hooks using the PARENT session's `session_id` — Delta
//! cannot tell from the id alone whether a tool call belongs to the parent or
//! to a subagent it spawned. Every hook payload, however, carries the JSONL
//! the hook is firing against (`transcript_path`), and for a nested subagent
//! that path is the subagent's own transcript (e.g.
//! `<parent-session>/subagents/agent-<id>.jsonl`), distinct from the parent's
//! `<parent-session>.jsonl`. Comparing the hook's `transcript_path` against
//! the session row's stored path is therefore the reliable way to detect —
//! and ignore — a nested subagent's tool calls.
//!
//! Without filtering, a nested `Agent` launch's `PreToolUse` would add a
//! `RunningSubagent` entry to the parent session's runtime, but the
//! completion `<task-notification>` lands in the nested subagent's own
//! transcript (which Delta never tails for the parent), so the running
//! indicator would stay lit forever.

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Whether `hook_transcript_path` belongs to a nested subagent rather than
    /// this session's own transcript.
    ///
    /// Returns `true` only in the unambiguous "nested subagent hook" case:
    /// the session row has a recorded `transcript_path` AND it differs from
    /// `hook_transcript_path`. Returns `false` when:
    ///
    /// - the session row has no recorded transcript path yet (nothing to
    ///   compare against — `SessionStart` is what fills it in, and a stray
    ///   hook arriving before that is a separate concern), or
    /// - the session row is absent entirely (an unregistered session — the
    ///   downstream handler will register it on its own normal path), or
    /// - the paths match (the common case — a hook for the parent itself).
    pub(in crate::interactor) async fn is_foreign_transcript(
        &self,
        hook_transcript_path: &str,
    ) -> Result<bool> {
        let Some(session) = self.store.session(self.id).await? else {
            return Ok(false);
        };
        let Some(stored) = session.transcript_path else {
            return Ok(false);
        };
        Ok(stored != hook_transcript_path)
    }
}
