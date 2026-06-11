use delta_model::SessionId;

use crate::error::Result;
use crate::open_sessions::PendingSpawn;
use crate::pane_token::PaneToken;
use crate::ports::{pane_for, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

use super::{SESSION_COMMAND, SESSION_ID_FLAG, SETTINGS_FLAG};

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Spawn a fresh session, optionally dispatching a first prompt.
    ///
    /// Mints a token and a fresh Claude `session_id` (a time-ordered UUID v7, so
    /// session ids sort chronologically by creation time), launches
    /// `claude --settings <path> --session-id <uuid>` in the launch directory,
    /// and records a
    /// [`PendingSpawn`] carrying that minted id (the binding key) and
    /// `first_prompt`. Pinning the id up front means the first `UserPromptSubmit`
    /// hook reports exactly this id, so the spawn correlates to its session by id
    /// rather than by working directory.
    ///
    /// When a `first_prompt` is present (a composer-initiated New), it is passed
    /// to `claude` as a trailing positional argument on the launch command line
    /// (`claude … <prompt>`) rather than typed into the pane after launch. An
    /// interactive `claude` invoked with a positional prompt auto-submits it at
    /// startup, which fires the `UserPromptSubmit` hook that binds this spawn —
    /// the hook then writes the deferred `pending_send` row that lets the first
    /// user line correlate. Submitting at launch avoids the failure mode of
    /// injecting keystrokes after a fixed settle delay: on a slow cold start the
    /// TUI input is not yet ready when the keystrokes land, they are lost, the
    /// prompt is never submitted, and the spawn sits pending forever. The command
    /// is forwarded as an argv tail (no shell), so a multi-line or quoted prompt
    /// is already safe. Returns the minted token.
    ///
    /// The registry lock is taken only for the brief record/rollback steps, never
    /// across the tmux/workspace I/O (which includes the create-session settle
    /// delay), so a spawn does not serialize concurrent registry readers (hooks,
    /// the PTY bridge) for the whole spawn duration. The `PendingSpawn` is
    /// recorded *before* `create_session` launches `claude`, so the
    /// `UserPromptSubmit` that the launch-submitted prompt triggers always finds a
    /// spawn to bind rather than racing ahead and being misread as external input.
    /// With the prompt on the command line the hook fires very soon after launch,
    /// so this pre-launch ordering — not any delay inside `create_session` — is
    /// what guarantees the spawn record already exists when the hook arrives. A
    /// failed `create_session` rolls the just-recorded pending back, so no
    /// dangling spawn is left behind.
    ///
    /// When `workdir` is `Some`, it is a user-selected path: it is validated and
    /// canonicalized via [`Workspace::resolve_existing_dir`] *before* anything is
    /// minted or launched, so an invalid path fails cleanly with no token, no
    /// pane, and no pending spawn left behind (mirroring the resume gate in
    /// [`Self::open_session`]). When `None`, the spawn falls back to its default
    /// per-token `<base>/<token>` directory.
    pub(in crate::interactor) async fn spawn_fresh(
        &self,
        first_prompt: Option<String>,
        workdir: Option<String>,
    ) -> Result<PaneToken> {
        // Validate a user-selected workdir before minting or launching anything,
        // so an invalid path is rejected with no side effects. The canonical
        // path becomes the launch directory; `None` defers to `<base>/<token>`
        // computed after the token is minted, below.
        let requested_workdir = match workdir {
            Some(dir) => Some(self.workspace.resolve_existing_dir(&dir).await?),
            None => None,
        };

        // Captured for tracing before `first_prompt` is moved into the spawn
        // record below.
        let has_first_prompt = first_prompt.is_some();

        // The minter is atomic, so token uniqueness needs no lock here.
        let token = self.mint_free_token().await?;
        let workdir = requested_workdir.unwrap_or_else(|| self.workdir_for(&token));
        let pane = pane_for(token.as_str());

        // Mint and pin the conversation's session id up front. `claude
        // --session-id <uuid>` makes the first `UserPromptSubmit` hook report
        // exactly this id, so the spawn correlates to its session by id rather
        // than by working directory. The id is a time-ordered UUID v7 (a 48-bit
        // millisecond timestamp prefix followed by random bits), so session ids
        // sort chronologically by creation time while remaining a fully valid
        // RFC 9562 UUID, and collision with an existing stored session is
        // astronomically unlikely.
        let session_id = SessionId::from(uuid::Uuid::now_v7().to_string());

        self.workspace
            .write_session_settings(&self.session_settings_path, &self.session_settings_json)
            .await?;
        let mut command = vec![
            SESSION_COMMAND.to_owned(),
            SETTINGS_FLAG.to_owned(),
            self.session_settings_path.clone(),
            SESSION_ID_FLAG.to_owned(),
            session_id.as_str().to_owned(),
        ];
        // Carry the first prompt on the launch command line as a trailing
        // positional argument. `claude` auto-submits a positional prompt at
        // startup, so the prompt is delivered without any post-launch keystroke
        // injection (which is lost when the TUI input is not yet ready on a slow
        // cold start). The argv tail is forwarded without a shell, so a
        // multi-line or quoted prompt is safe.
        if let Some(text) = first_prompt.clone() {
            command.push(text);
        }

        // Record the spawn *before* launching `claude`, so the `UserPromptSubmit`
        // that the launch-submitted prompt triggers finds a pending spawn to bind
        // instead of racing ahead and being misread as external input. With the
        // prompt on the command line the hook fires very soon after launch, so
        // this ordering — not any delay inside `create_session` — is what makes
        // the spawn record reliably present when the hook arrives.
        self.open_sessions.lock().await.push_pending(PendingSpawn {
            token: token.clone(),
            pane: pane.clone(),
            session_id: session_id.clone(),
            workdir: workdir.clone(),
            first_prompt,
            // Stamp the spawn for the watchdog deadline. From here the only thing
            // that binds it is the first `UserPromptSubmit` hook; if that never
            // arrives, the reaper uses this instant to reap the stuck spawn.
            created_at: std::time::Instant::now(),
        });

        // Launch the session. If `create_session` fails, the spawn never starts,
        // so roll the just-recorded pending back (otherwise a later, unrelated
        // `UserPromptSubmit` could mis-bind to this abandoned pane) and surface
        // the error.
        if let Err(spawn_err) = self
            .tmux
            .create_session(token.as_str(), &workdir, &command)
            .await
        {
            tracing::error!(
                token = %token.as_str(),
                session_id = %session_id,
                error = %spawn_err,
                "fresh spawn failed to launch; rolling back the pending spawn"
            );
            self.open_sessions
                .lock()
                .await
                .remove_pending_for_token(&token);
            return Err(spawn_err);
        }
        tracing::info!(
            token = %token.as_str(),
            session_id = %session_id,
            workdir = %workdir,
            has_first_prompt = has_first_prompt,
            "fresh spawn launched; awaiting first UserPromptSubmit to bind"
        );
        Ok(token)
    }
}
