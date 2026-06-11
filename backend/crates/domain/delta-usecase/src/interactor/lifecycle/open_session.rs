use std::time::Instant;

use delta_model::SessionId;

use crate::error::{Error, Result};
use crate::open_sessions::{OpenHandle, ResumingSession};
use crate::pane_token::PaneToken;
use crate::ports::{pane_for, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

use super::{RESUME_FLAG, SESSION_COMMAND, SETTINGS_FLAG};

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Resume a closed but known session under a fresh tmux session.
    ///
    /// The conversational `session_id` is known up front, so this mints a fresh
    /// token, re-writes the settings file (at Delta's own path, not the session
    /// cwd; the port is idempotent), launches `claude --settings <file> --resume
    /// <id>` in the stored cwd, and binds the new pane to `id` immediately.
    /// Resuming an already-open session is a no-op
    /// that returns the existing handle's token (the double-open guard).
    ///
    /// It does **not** dispatch the first prompt here. `claude --resume` needs a
    /// couple of seconds to replay the transcript before its TUI can accept input
    /// — far longer than any safe fixed settle — so the resumed pane is recorded
    /// as not-yet-ready ([`OpenSessions::start_resuming`]) and the caller's first
    /// keystroke is held by `enqueue_into_open` until the resume's
    /// `SessionStart(source=resume)` fires (which only *marks* it ready, because
    /// that hook blocks `claude` until it returns) and is then typed a beat later
    /// by `dispatch_ready_resumes` on the background tick, once `claude` has left
    /// the hook and is input-ready. This closes the stall where the first
    /// keystroke landed before the cold pane was ready and was silently lost (no
    /// `UserPromptSubmit`, stuck "pending" forever). A resume that never becomes
    /// ready is failed by the watchdog (see [`Self::reap_stale_spawns`]).
    ///
    /// [`OpenSessions::start_resuming`]: crate::open_sessions::OpenSessions::start_resuming
    ///
    /// Before returning, the existing transcript is synced so the DB's message
    /// rows and read cursor catch up to whatever Claude Code already wrote for
    /// this conversation. This matters because the resume's first
    /// `UserPromptSubmit` resolves thread context from already-persisted history:
    /// [`Self::thread_switch_context`] reads [`SessionStore::latest_user_thread`]
    /// and [`Self::sync_transcript`] seeds `carry_thread` from it. If the DB were
    /// behind the transcript at that first prompt (a cold/just-restored DB, or
    /// any DB-behind-transcript state), `latest_user_thread` would report `None`,
    /// mis-seeding `carry_thread` to `main` and mis-attributing any leading
    /// non-user line of the resumed batch. Catching up here, before
    /// `claude --resume` can produce a new prompt hook, makes the user's actual
    /// last thread visible on that first prompt. The sync is `sync_lock`-guarded
    /// and cursor-based idempotent, so it never double-ingests.
    pub async fn open_session(&self, id: &SessionId) -> Result<PaneToken> {
        let session = self
            .store
            .session(id)
            .await?
            .ok_or_else(|| Error::SessionNotFound(id.as_str().to_owned()))?;

        let token = {
            let mut registry = self.open_sessions.lock().await;
            // Double-open guard: if already open, route to the existing pane.
            if let Some(handle) = registry.handle(id) {
                return Ok(handle.token.clone());
            }

            // Resume gate: `claude --resume <id>` replays from the local JSONL
            // transcript, so a missing transcript makes resume impossible. tmux
            // would still report a clean spawn (it only checks `new-session`'s
            // exit code, which is 0 before claude's own resume failure surfaces),
            // leaving the UI stuck on a "waiting" pending row that never clears.
            // Refuse here — before minting a token, writing settings, or spawning
            // — so no pane is created and no optimistic pending send is enqueued.
            if !self.transcript.exists(&session.transcript_path).await? {
                return Err(Error::ResumeUnavailable(id.as_str().to_owned()));
            }

            let token = self.mint_free_token().await?;
            let workdir = session.cwd.clone();
            // Re-write the settings file before resuming, in case the port is
            // fresh or the file was lost. It lives at a Delta-owned path, not in
            // `workdir`, so resuming in a real project never touches that
            // project's own `.claude/settings.json`.
            self.workspace
                .write_session_settings(&self.session_settings_path, &self.session_settings_json)
                .await?;
            let command = vec![
                SESSION_COMMAND.to_owned(),
                SETTINGS_FLAG.to_owned(),
                self.session_settings_path.clone(),
                RESUME_FLAG.to_owned(),
                id.as_str().to_owned(),
            ];
            self.tmux
                .create_session(token.as_str(), &workdir, &command)
                .await?;
            let pane = pane_for(token.as_str());
            registry.bind(
                id.clone(),
                OpenHandle {
                    token: token.clone(),
                    pane: pane.clone(),
                    workdir,
                },
            );
            // Record the resume as not-yet-ready: the pane is bound, but the
            // first prompt is held until `SessionStart(source=resume)` confirms
            // the cold TUI can accept input. `enqueue_into_open` parks the
            // keystroke here (via `hold_first_prompt`); `SessionStart(resume)`
            // marks it ready and `dispatch_ready_resumes` types it a beat later on
            // the background tick; the watchdog fails the resume if readiness
            // never arrives. A resume with no following send leaves `held_prompt`
            // `None` — readiness just clears the gate. `ready_at` starts `None`
            // (not yet ready).
            registry.start_resuming(
                id.clone(),
                ResumingSession {
                    token: token.clone(),
                    pane,
                    held_prompt: None,
                    created_at: Instant::now(),
                    ready_at: None,
                },
            );
            token
        };

        // Catch the DB up to the existing transcript before the resume's first
        // prompt can arrive, so thread context resolves against the user's real
        // last thread rather than a DB-behind `None`. Released the registry lock
        // above first: `sync_transcript` takes its own `sync_lock` and does not
        // need the registry held.
        self.sync_transcript(&session).await?;
        Ok(token)
    }
}
