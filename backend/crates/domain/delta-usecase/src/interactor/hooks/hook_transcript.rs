//! Classifying the transcript a Claude Code hook fires against, and following
//! the session's transcript when Claude Code moves it.
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
//!   worktree, Claude Code moves its JSONL (and the session directory beside
//!   it) to the project directory of the new working directory, appends a
//!   `{"type":"relocated","relocatedCwd":…}` line, and reports the new path in
//!   every later hook. The file is moved whole, not copied, so the old path
//!   stops existing. Delta follows the move: it re-points the session row at
//!   the new file and keeps reading where it left off.
//!
//! Before relocations existed, "differs from the stored path" was enough to
//! call a hook foreign. It no longer is — a relocated session's own hooks
//! differ too — so foreignness is decided by the subagent location instead.

use std::path::Path;

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, TranscriptRead, Workspace};

use super::validate_transcript_path;

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

/// Where to resume reading a moved transcript, given the cursor Delta kept for
/// the old file, the uuid of the last line it ingested, and a full read of the
/// new file.
///
/// Claude Code moves the file whole and only appends after the move, so the
/// cursor normally carries over unchanged. It is trusted when the last
/// ingested line sits before it in the new file and the file is at least that
/// long. Otherwise — the new file is shorter than the cursor, or the cursor
/// sits before the last ingested line (the background tail used to pull it
/// back on reading the moved-away file as empty) — reading resumes right
/// after the last ingested line, located by its uuid. Nothing at or before
/// that line is read again, so no message is duplicated and no turn effect is
/// replayed. When the new file holds no line Delta ingested, none of it has
/// been seen, so it is read from the start.
fn resume_line(cursor: usize, last_ingested: Option<&str>, new_file: &TranscriptRead) -> usize {
    let Some(uuid) = last_ingested else {
        // Nothing ingested yet: nothing can be duplicated either way, so keep
        // the cursor unless the file cannot even reach it.
        return if cursor <= new_file.total_lines {
            cursor
        } else {
            0
        };
    };
    match new_file
        .messages
        .iter()
        .find(|message| message.uuid.as_str() == uuid)
    {
        Some(message) => {
            let after = usize::try_from(message.seq).map_or(0, |seq| seq + 1);
            if after <= cursor && cursor <= new_file.total_lines {
                cursor
            } else {
                after
            }
        }
        None => 0,
    }
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

    /// Re-point the session at the transcript Claude Code moved it to.
    ///
    /// The new path is confined exactly as a registering hook's is
    /// ([`validate_transcript_path`]); a path Delta refuses is never stored,
    /// the session keeps its old path, and this returns `false`. Otherwise the
    /// line cursor is reconciled
    /// against the new file (see [`resume_line`]), and the session row takes
    /// the new path plus the working directory the file's `relocated` line
    /// names, so a later `claude --resume` runs where Claude Code now files the
    /// transcript. Every reader of the transcript — the hook syncs, the
    /// background tail, close and resume — reads the path from the row, so they
    /// all follow the new file from here on, and this returns `true`.
    ///
    /// Two callers find the moved file: a hook reporting its path
    /// ([`Self::admit_hook_transcript`]), and a resume whose stored transcript
    /// is gone and which looks the session's file up under the transcript root
    /// (see `open_session`).
    pub(in crate::interactor) async fn follow_relocated_transcript(
        &mut self,
        stored: &str,
        relocated: &str,
    ) -> Result<bool> {
        if let Some(root) = &self.transcript_root {
            if let Err(err) = validate_transcript_path(root, relocated) {
                tracing::warn!(
                    session_id = %self.id,
                    stored = %stored,
                    reported = %relocated,
                    error = %err,
                    "refusing to follow a relocated transcript; keeping the stored one"
                );
                return Ok(false);
            }
        }

        let new_file = self.transcript.read_from(relocated, 0).await?;
        let cursor = self.store.transcript_lines_read(self.id).await?;
        let last_ingested = self.store.latest_message_uuid(self.id).await?;
        let resume_at = resume_line(
            cursor,
            last_ingested.as_ref().map(|uuid| uuid.as_str()),
            &new_file,
        );
        if resume_at != cursor {
            tracing::info!(
                session_id = %self.id,
                cursor,
                resume_at,
                new_file_lines = new_file.total_lines,
                "relocated transcript does not continue at the kept line cursor; \
                 resuming right after the last ingested line"
            );
            self.store
                .set_transcript_lines_read(self.id, resume_at)
                .await?;
        }

        let relocated_cwd = new_file.relocated_cwd.as_deref();
        self.store
            .relocate_transcript(self.id, relocated, relocated_cwd)
            .await?;
        tracing::info!(
            session_id = %self.id,
            from = %stored,
            to = %relocated,
            cwd = relocated_cwd.unwrap_or("(unchanged)"),
            "Claude Code relocated the session's transcript; following it"
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactor::testing::assistant_line;

    fn new_file(uuids: &[&str], total_lines: usize) -> TranscriptRead {
        TranscriptRead {
            messages: uuids
                .iter()
                .enumerate()
                .map(|(seq, uuid)| {
                    let mut line = assistant_line(uuid, "x");
                    line.seq = seq as i64;
                    line
                })
                .collect(),
            total_lines,
            relocated_cwd: None,
        }
    }

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

    /// The moved file holds every line the old one had, then the `relocated`
    /// line: the cursor carries over unchanged.
    #[test]
    fn a_moved_file_keeps_the_cursor() {
        let file = new_file(&["u0", "u1", "u2"], 5);
        assert_eq!(resume_line(4, Some("u2"), &file), 4);
    }

    /// The tail read the moved-away file as empty and pulled the cursor back:
    /// reading resumes right after the last ingested line, not from the top.
    #[test]
    fn a_cursor_behind_the_last_ingested_line_resumes_after_it() {
        let file = new_file(&["u0", "u1", "u2"], 5);
        assert_eq!(resume_line(0, Some("u1"), &file), 2);
    }

    #[test]
    fn a_file_shorter_than_the_cursor_resumes_after_the_last_ingested_line() {
        let file = new_file(&["u0", "u1"], 2);
        assert_eq!(resume_line(9, Some("u1"), &file), 2);
    }

    /// Nothing Delta ingested is in the new file, so none of it was seen.
    #[test]
    fn a_file_without_the_last_ingested_line_is_read_from_the_start() {
        let file = new_file(&["v0", "v1"], 2);
        assert_eq!(resume_line(9, Some("u1"), &file), 0);
    }

    #[test]
    fn with_nothing_ingested_the_cursor_is_kept_when_the_file_reaches_it() {
        let file = new_file(&[], 3);
        assert_eq!(resume_line(2, None, &file), 2);
        assert_eq!(resume_line(7, None, &file), 0);
    }
}
