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
    /// rather than by working directory. When a `first_prompt` is present (a
    /// composer-initiated New), it is typed into the freshly-created pane so
    /// Claude actually receives the message and fires the `UserPromptSubmit` hook
    /// that binds this spawn — the hook then writes the deferred `pending_send`
    /// row that lets the first user line correlate. Returns the minted token.
    ///
    /// The registry lock is taken only for the brief record/rollback steps, never
    /// across the tmux/workspace I/O (which includes the create-session settle
    /// delay), so a spawn does not serialize concurrent registry readers (hooks,
    /// the PTY bridge) for the whole spawn duration. The `PendingSpawn` is
    /// recorded *before* the first prompt is dispatched, so the
    /// `UserPromptSubmit` that prompt triggers always finds a spawn to bind
    /// rather than racing ahead and being misread as external input.
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
        let command = vec![
            SESSION_COMMAND.to_owned(),
            SETTINGS_FLAG.to_owned(),
            self.session_settings_path.clone(),
            SESSION_ID_FLAG.to_owned(),
            session_id.as_str().to_owned(),
        ];
        self.tmux
            .create_session(token.as_str(), &workdir, &command)
            .await?;

        // Record the spawn before dispatching the first prompt, so the hook the
        // prompt triggers can bind it. (A failed create above returns early with
        // nothing recorded, so no dangling pending spawn is left behind.)
        self.open_sessions.lock().await.push_pending(PendingSpawn {
            token: token.clone(),
            pane: pane.clone(),
            session_id,
            workdir,
            first_prompt: first_prompt.clone(),
        });

        // Type the deferred first prompt into the new pane. If it never reaches
        // the pane the spawn would sit idle forever (Claude never fires the hook
        // that binds it), so roll the pending spawn back and surface the error.
        if let Some(text) = first_prompt {
            if let Err(dispatch_err) = self.tmux.send_line(&pane, &text).await {
                self.open_sessions
                    .lock()
                    .await
                    .remove_pending_for_token(&token);
                return Err(dispatch_err);
            }
        }
        Ok(token)
    }
}
