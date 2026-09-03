use delta_model::{Session, SessionId, ThreadId};

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{
    GitWorktree, NewSession, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace,
};

use super::validate_transcript_path;

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Upsert the session row and emit [`SessionEvent::SessionRegistered`],
    /// returning the session and its `main` thread id.
    ///
    /// For a Delta launch the row already exists as `spawning` (written when the
    /// id was minted), so this *activates* it — `spawning` → `active`, filling
    /// in the hook-reported transcript path; for an external `claude` the row is
    /// inserted fresh (see [`SessionStore::register_session`]).
    ///
    /// Takes the raw identifying fields rather than a specific hook type, because
    /// two hooks register a session — the first `UserPromptSubmit` and
    /// `SessionStart(source=startup)` — and both carry `session_id`, `cwd`, and
    /// `transcript_path`. `register_session` is idempotent for an already-active
    /// id, so a second call is harmless; the event is still emitted (the browser
    /// already invalidates its list idempotently on it).
    pub(in crate::interactor::hooks) async fn register_session_row(
        &self,
        session_id: &SessionId,
        cwd: &str,
        transcript_path: &str,
        events: &mut Vec<SessionEvent>,
    ) -> Result<(Session, ThreadId)> {
        // Confine the hook-reported transcript path before it is persisted. This
        // is the single choke point every registering hook (the first
        // `UserPromptSubmit` and `SessionStart(startup)`) funnels through, so a
        // path Delta refuses here is never stored and never reaches the tailer's
        // `fs::read_to_string`.
        if let Some(root) = &self.transcript_root {
            validate_transcript_path(root, transcript_path)?;
        }
        let (session, main_id) = self
            .store
            .register_session(NewSession {
                id: session_id.clone(),
                cwd: cwd.to_owned(),
                transcript_path: transcript_path.to_owned(),
                // The hook-driven activate path knows nothing of Delta's
                // launch context: the spawn-time snapshot is recorded once by
                // `insert_spawning_session` and is left untouched here. For
                // an externally-started `claude` (the fresh-insert side of
                // `register_session`) Delta likewise has no launch git
                // context, so all three stay `None`. The same holds for the
                // session's `pull_request_number`, which this path does not
                // carry at all: it is written only by the spawning insert.
                branch_at_launch: None,
                repo_root: None,
                repository_display_name: None,
            })
            .await?;
        events.push(SessionEvent::SessionRegistered {
            session_id: session_id.clone(),
        });
        Ok((session, main_id))
    }
}
