use delta_model::Send;

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::PendingSpawn;
use crate::pane_token::PaneToken;
use crate::ports::{pane_for, SessionStore, TmuxDriver, Transcript, Workspace};

use super::{SESSION_ID_FLAG, SETTINGS_FLAG};

/// The result of a fresh spawn: the launch's pane token and — when the spawn
/// carried a first prompt — the already-enqueued `send` row for it (which
/// names the eagerly-created session row and its `main` thread).
pub(in crate::interactor) struct FreshSpawn {
    pub token: PaneToken,
    /// The `dispatched` send row for the first prompt, written before the
    /// launch; `None` for a prompt-less plain spawn.
    pub first_send: Option<Send>,
}

impl<T, X, S, W> SessionContext<'_, T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Spawn this freshly-minted session's pane, optionally dispatching a
    /// first prompt.
    ///
    /// The routing layer minted the session id (a time-ordered UUID v7) and
    /// spawned this actor for it; pinning the id up front via
    /// `claude --session-id <uuid>` means the first `UserPromptSubmit` hook
    /// reports exactly this id, so the launch's hooks route straight back to
    /// this actor — correlation by id, never by working directory.
    ///
    /// This **eagerly inserts the session row** (status `spawning`, transcript
    /// path unknown until the first hook) with its `main` thread, enqueues the
    /// first prompt's `send` row when one is given, and only then launches
    /// `claude --settings <path> --session-id <uuid>` in the launch directory.
    /// Because the row and the send exist before the launch, the REST response
    /// for a composer-initiated New carries real ids instead of placeholders,
    /// and the first `UserPromptSubmit` correlates through the normal
    /// single-outstanding machinery with no bind-time row writing.
    ///
    /// A [`PendingSpawn`] is recorded on this actor's runtime state: the first
    /// hook *activates* the eager row (`spawning` → `active`, filling the
    /// transcript path) via [`SessionRuntime::bind_pending_spawn`].
    ///
    /// When a `first_prompt` is present (a composer-initiated New), it is passed
    /// to `claude` as a trailing positional argument on the launch command line
    /// (`claude … <prompt>`) rather than typed into the pane after launch. An
    /// interactive `claude` invoked with a positional prompt auto-submits it at
    /// startup, which fires the `UserPromptSubmit` hook that binds this spawn.
    /// Submitting at launch avoids the failure mode of injecting keystrokes
    /// after a fixed settle delay: on a slow cold start the TUI input is not yet
    /// ready when the keystrokes land, they are lost, the prompt is never
    /// submitted, and the spawn sits pending forever. The command is forwarded
    /// as an argv tail (no shell), so a multi-line or quoted prompt is already
    /// safe.
    ///
    /// The `PendingSpawn` is recorded *before* `create_session` launches
    /// `claude`, so the `UserPromptSubmit` (or `SessionStart`) that the launch
    /// triggers always finds a spawn to bind rather than racing ahead and being
    /// misread as external input — those hooks land on this same mailbox,
    /// strictly after this message. A failed `create_session` rolls back both
    /// the just-recorded pending *and* the eager session row (the cascade
    /// removes its send), so no dangling spawn or orphan row is left behind.
    ///
    /// When `workdir` is `Some`, it is a user-selected path: it is validated and
    /// canonicalized via [`Workspace::resolve_existing_dir`] *before* anything is
    /// minted or launched, so an invalid path fails cleanly with no token, no
    /// pane, and no pending spawn left behind (mirroring the resume gate in
    /// [`Self::open_session`]). When `None`, the spawn falls back to its default
    /// per-token `<base>/<token>` directory.
    ///
    /// [`SessionRuntime::bind_pending_spawn`]: crate::interactor::session_actor::runtime::SessionRuntime::bind_pending_spawn
    pub(in crate::interactor) async fn spawn_fresh(
        &mut self,
        first_prompt: Option<String>,
        workdir: Option<String>,
    ) -> Result<FreshSpawn> {
        let session_id = self.id.clone();
        // Validate a user-selected workdir before minting or launching anything,
        // so an invalid path is rejected with no side effects. The canonical
        // path becomes the launch directory; `None` defers to `<base>/<token>`
        // computed after the token is minted, below.
        let requested_workdir = match workdir {
            Some(dir) => Some(self.workspace.resolve_existing_dir(&dir).await?),
            None => None,
        };

        // The minter is atomic, so token uniqueness needs no coordination here.
        let token = self.mint_free_token().await?;
        let workdir = requested_workdir.unwrap_or_else(|| self.workdir_for(&token));
        let pane = pane_for(token.as_str());

        // Eagerly create the session row and its `main` thread, then the first
        // prompt's send row bound to those real ids. Hooks cannot arrive before
        // the launch below (and would queue behind this message anyway), so
        // nothing races this write; if the launch fails the row is deleted
        // again in the rollback.
        let (_session, main_thread_id) = self
            .store
            .insert_spawning_session(&session_id, &workdir)
            .await?;
        let first_send = match first_prompt.as_deref() {
            Some(text) => Some(
                self.store
                    .enqueue_send(&session_id, main_thread_id, None, text, None)
                    .await?,
            ),
            None => None,
        };
        // The first prompt is delivered on the launch command line below, so it
        // is already "dispatched": move the turn machine to `AwaitingEcho` now,
        // before the launch, so the first `UserPromptSubmit` the auto-submitted
        // prompt fires always finds the dispatch recorded.
        if let Some(send) = &first_send {
            self.apply_turn_input(crate::turn::TurnInput::Dispatch { send_id: send.id })
                .await?;
        }

        self.workspace
            .write_session_settings(&self.session_settings_path, &self.session_settings_json)
            .await?;
        let mut command = vec![
            self.launch.claude_bin.clone(),
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
        self.state.push_pending(PendingSpawn {
            token: token.clone(),
            pane: pane.clone(),
            // Stamp the spawn for the watchdog deadline. From here the only thing
            // that binds it is the first `UserPromptSubmit` hook; if that never
            // arrives, the reaper uses this instant to reap the stuck spawn.
            created_at: std::time::Instant::now(),
        });

        // Launch the session. If `create_session` fails, the spawn never starts,
        // so roll back the just-recorded pending (otherwise a later, unrelated
        // hook could mis-bind to this abandoned pane) and the eager session row
        // (the cascade removes its main thread and first send), then surface
        // the error. The REST caller gets the failure synchronously, so no
        // `SpawnFailed` event is needed for this path.
        if let Err(spawn_err) = self
            .tmux
            .create_session(token.as_str(), &workdir, &command)
            .await
        {
            tracing::error!(
                token = %token.as_str(),
                session_id = %session_id,
                error = %spawn_err,
                "fresh spawn failed to launch; rolling back the pending spawn and \
                 the eager session row"
            );
            self.state.remove_pending_for_token(&token);
            // The session row (and its first send, by cascade) is deleted, so
            // the turn entry is dropped without orphan handling.
            self.state.forget_turn();
            self.store.delete_session(&session_id).await?;
            return Err(spawn_err);
        }
        tracing::info!(
            token = %token.as_str(),
            session_id = %session_id,
            workdir = %workdir,
            has_first_prompt = first_send.is_some(),
            "fresh spawn launched; awaiting first UserPromptSubmit to bind"
        );
        Ok(FreshSpawn { token, first_send })
    }
}
