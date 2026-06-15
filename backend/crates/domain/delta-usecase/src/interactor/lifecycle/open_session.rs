use std::time::Instant;

use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::{OpenHandle, ResumingSession};
use crate::pane_token::PaneToken;
use crate::ports::{pane_for, GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::{RESUME_FLAG, SETTINGS_FLAG};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Resume a closed but known session under a fresh tmux session.
    ///
    /// The conversational `session_id` is known up front, so this mints a fresh
    /// token, re-writes the settings file (at Delta's own path, not the session
    /// cwd; the port is idempotent), launches `claude --settings <file> --resume
    /// <id>` in the stored cwd, and binds the new pane to the session
    /// immediately. Resuming an already-open session is a no-op
    /// that returns the existing handle's token (the double-open guard).
    ///
    /// It does **not** dispatch the first prompt here. `claude --resume` needs a
    /// couple of seconds to replay the transcript before its TUI can accept input
    /// — far longer than any safe fixed settle — so the resumed pane is recorded
    /// as not-yet-ready ([`SessionRuntime::start_resuming`]) and the caller's
    /// first keystroke is held by `enqueue_into_open` until the resume's
    /// `SessionStart(source=resume)` fires (which only *marks* it ready, because
    /// that hook blocks `claude` until it returns) and is then typed a beat later
    /// by the resume tick, once `claude` has left the hook and is input-ready.
    /// This closes the stall where the first keystroke landed before the cold
    /// pane was ready and was silently lost (no `UserPromptSubmit`, stuck
    /// "pending" forever). A resume that never becomes ready is failed by the
    /// watchdog (see the reap tick).
    ///
    /// [`SessionRuntime::start_resuming`]: crate::interactor::session_actor::runtime::SessionRuntime::start_resuming
    ///
    /// Before returning, the existing transcript is synced so the DB's message
    /// rows and read cursor catch up to whatever Claude Code already wrote for
    /// this conversation. This matters because the resume's first
    /// `UserPromptSubmit` resolves thread context from already-persisted history:
    /// `thread_switch_context` reads [`SessionStore::latest_user_thread`]
    /// and [`Self::sync_transcript`] seeds `carry_thread` from it. If the DB were
    /// behind the transcript at that first prompt (a cold/just-restored DB, or
    /// any DB-behind-transcript state), `latest_user_thread` would report `None`,
    /// mis-seeding `carry_thread` to `main` and mis-attributing any leading
    /// non-user line of the resumed batch. Catching up here, before
    /// `claude --resume` can produce a new prompt hook, makes the user's actual
    /// last thread visible on that first prompt. The sync is cursor-based
    /// idempotent (and mailbox-ordered), so it never double-ingests.
    pub(in crate::interactor) async fn open_session(&mut self) -> Result<PaneToken> {
        let id = self.id;
        let session = self
            .store
            .session(id)
            .await?
            .ok_or_else(|| Error::SessionNotFound(id.as_str().to_owned()))?;

        // Double-open guard: if already open, route to the existing pane.
        if let Some(handle) = self.state.handle() {
            return Ok(handle.token.clone());
        }

        // Resume gate: `claude --resume <id>` replays from the local JSONL
        // transcript, so a missing transcript makes resume impossible. tmux
        // would still report a clean spawn (it only checks `new-session`'s
        // exit code, which is 0 before claude's own resume failure surfaces),
        // leaving the UI stuck on a "waiting" pending row that never clears.
        // Refuse here — before minting a token, writing settings, or spawning
        // — so no pane is created and no optimistic send is enqueued.
        // A session still `spawning` has no transcript path at all (the
        // first hook never bound it), so it is equally unresumable.
        let resumable = match session.transcript_path.as_deref() {
            Some(path) => self.transcript.exists(path).await?,
            None => false,
        };
        if !resumable {
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
            self.launch.claude_bin.clone(),
            SETTINGS_FLAG.to_owned(),
            self.session_settings_path.clone(),
            RESUME_FLAG.to_owned(),
            id.as_str().to_owned(),
        ];
        // Pre-accept Claude Code's workspace-trust dialog when resuming into a
        // git repository (a worktree session, or any real-repo cwd): a fresh
        // pane resuming there would otherwise stall on the interactive trust
        // dialog. Seed before launching so a failure aborts the resume cleanly
        // with no pane created. The default `<base>/<token>` scratch dir is not a
        // git repo, so `repo_root` returns `None` and seeding is skipped.
        if self.git_worktree.repo_root(&workdir).await?.is_some() {
            self.git_worktree.ensure_dir_trusted(&workdir).await?;
        }
        self.tmux
            .create_session(token.as_str(), &workdir, &command)
            .await?;
        let pane = pane_for(token.as_str());
        self.state.bind(OpenHandle {
            token: token.clone(),
            pane: pane.clone(),
        });
        // Record the resume as not-yet-ready: the pane is bound, but the
        // first prompt is held until `SessionStart(source=resume)` confirms
        // the cold TUI can accept input. `enqueue_into_open` parks the
        // keystroke here (via `hold_first_prompt`); `SessionStart(resume)`
        // marks it ready and the resume tick types it a beat later; the
        // watchdog fails the resume if readiness never arrives. A resume with
        // no following send leaves `held_prompt` `None` — readiness just
        // clears the gate. `ready_at` starts `None` (not yet ready).
        self.state.start_resuming(ResumingSession {
            token: token.clone(),
            pane,
            held_prompt: None,
            created_at: Instant::now(),
            ready_at: None,
        });

        // The session was closed until this resume, and a closed session has no
        // turn in flight — but its last life may have left stale turn state
        // behind (e.g. a `claude` that ended mid-turn without ever delivering a
        // `Stop`). Feed `Close` now so the resumed session starts from a clean
        // `Idle` (any stale outstanding send is swept) instead of deferring its
        // first prompt behind a phantom turn forever.
        self.apply_turn_input(crate::turn::TurnInput::Close).await?;

        // Catch the DB up to the existing transcript before the resume's first
        // prompt can arrive, so thread context resolves against the user's real
        // last thread rather than a DB-behind `None`. The sync runs on this
        // same actor, so it is already ordered against the hooks.
        self.sync_transcript(&session).await?;
        Ok(token)
    }
}
