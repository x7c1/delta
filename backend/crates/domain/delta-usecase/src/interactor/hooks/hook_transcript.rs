//! Classifying the transcript a Claude Code hook fires against.
//!
//! Every hook payload carries the JSONL the hook is firing against
//! (`transcript_path`). For the session itself that is its own transcript,
//! `<project dir>/<session id>.jsonl`. Two other shapes reach a session's hook
//! endpoint under the same `session_id`:
//!
//! - **A subagent's transcript.** Claude Code dispatches a subagent's
//!   `PreToolUse` / `PostToolUse` / `PermissionRequest` hooks under the
//!   PARENT session's id, but names the subagent's own JSONL, which lives in
//!   the parent's session directory: `<project dir>/<session id>/subagents/
//!   agent-<id>.jsonl`. Delta does not track that conversation, so the hook is
//!   foreign and its tool-call bookkeeping is dropped. Without that, a nested
//!   call could attach a permission row to the parent, or clear a parent's
//!   running subagent that happens to share its `tool_use_id`.
//! - **The session's own transcript, moved.** When a session enters a
//!   worktree, Claude Code moves its JSONL to another project directory and
//!   reports the new path in every later hook. Delta follows the move (see
//!   `follow_relocated_transcript`).
//!
//! Before relocations existed, "differs from the stored path" was enough to
//! call a hook foreign. It no longer is — a relocated session's own hooks
//! differ too — so foreignness is decided by the subagent location instead.

use std::path::Path;

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

/// The directory, inside a session's own directory, that holds its subagents'
/// transcripts.
const SUBAGENTS_DIR: &str = "subagents";

/// Whose transcript a hook fired against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::interactor) enum HookTranscript {
    /// The session's own transcript — the one Delta tails for it. A hook that
    /// named a moved transcript has already been followed by the time this is
    /// returned.
    Own,
    /// A transcript Delta does not tail for this session (a subagent's, or a
    /// path it cannot follow). The hook's tool-call bookkeeping is skipped.
    Foreign,
}

/// Whether `path` is a subagent's transcript: a file directly inside a
/// `subagents` directory. A session's own transcript sits directly in a
/// project directory, whose name is the munged working directory and never the
/// bare word `subagents`, so the two cannot be confused.
fn is_subagent_transcript(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|dir| dir == SUBAGENTS_DIR)
}

/// Whether `path` is named the way Claude Code names a session's own
/// transcript: `<session id>.jsonl`. A move keeps the file name, so this is
/// what makes a reported path recognisable as the same session's transcript.
fn names_session_transcript(path: &Path, session_id: &str) -> bool {
    path.file_stem().is_some_and(|stem| stem == session_id)
        && path.extension().is_some_and(|ext| ext == "jsonl")
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Classify the transcript a hook names, following the session's
    /// transcript first when the hook reports that Claude Code moved it.
    ///
    /// - A subagent's transcript is [`HookTranscript::Foreign`].
    /// - A session with no row, or no recorded path yet, has nothing to compare
    ///   against (`SessionStart` / the first `UserPromptSubmit` fill it in), so
    ///   the hook is [`HookTranscript::Own`] and its handler takes its normal
    ///   path.
    /// - The stored path is [`HookTranscript::Own`] — the common case.
    /// - A different `<session id>.jsonl` is the session's own transcript,
    ///   moved: Delta re-points the session at it (see
    ///   [`Self::follow_relocated_transcript`]) and the hook is
    ///   [`HookTranscript::Own`]. This is also what heals a live session that
    ///   was stuck on a moved-away path before Delta followed moves: its next
    ///   hook re-points it. A closed one receives no hook; resuming it finds
    ///   the moved file instead (see `open_session`).
    /// - Anything else is a shape Delta does not know, so it keeps following
    ///   the stored path and treats the hook as [`HookTranscript::Foreign`].
    pub(in crate::interactor) async fn admit_hook_transcript(
        &mut self,
        hook_transcript_path: &str,
    ) -> Result<HookTranscript> {
        let hook_path = Path::new(hook_transcript_path);
        if is_subagent_transcript(hook_path) {
            return Ok(HookTranscript::Foreign);
        }
        let Some(session) = self.store.session(self.id).await? else {
            return Ok(HookTranscript::Own);
        };
        let Some(stored) = session.transcript_path else {
            return Ok(HookTranscript::Own);
        };
        if stored == hook_transcript_path {
            return Ok(HookTranscript::Own);
        }
        if !names_session_transcript(hook_path, self.id.as_str()) {
            tracing::warn!(
                session_id = %self.id,
                stored = %stored,
                reported = %hook_transcript_path,
                "hook names a transcript that is neither the session's own nor a \
                 subagent's; ignoring it and keeping the stored transcript"
            );
            return Ok(HookTranscript::Foreign);
        }
        let followed = self
            .follow_relocated_transcript(&stored, hook_transcript_path)
            .await?;
        Ok(if followed {
            HookTranscript::Own
        } else {
            HookTranscript::Foreign
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real layout: a subagent's JSONL sits in the parent's session
    /// directory, under `subagents/`; the session's own JSONL sits beside that
    /// directory.
    #[test]
    fn a_file_under_subagents_is_a_subagent_transcript() {
        assert!(is_subagent_transcript(Path::new(
            "/p/-work/sess-1/subagents/agent-a1b2.jsonl"
        )));
        assert!(!is_subagent_transcript(Path::new("/p/-work/sess-1.jsonl")));
        // A project directory is a munged cwd, so even a cwd ending in
        // `subagents` does not produce a bare `subagents` directory.
        assert!(!is_subagent_transcript(Path::new(
            "/p/-work-subagents/sess-1.jsonl"
        )));
    }

    #[test]
    fn only_the_session_id_names_its_own_transcript() {
        assert!(names_session_transcript(
            Path::new("/p/-work-wt/sess-1.jsonl"),
            "sess-1"
        ));
        assert!(!names_session_transcript(
            Path::new("/p/-work-wt/sess-2.jsonl"),
            "sess-1"
        ));
        assert!(!names_session_transcript(
            Path::new("/p/-work-wt/sess-1.json"),
            "sess-1"
        ));
    }
}
