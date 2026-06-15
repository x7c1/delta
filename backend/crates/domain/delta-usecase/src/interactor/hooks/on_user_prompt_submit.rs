use delta_attribution::claude_format;

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{
    GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, UserPromptSubmitHook,
    Workspace,
};
use crate::turn::TurnInput;

use super::match_uuid_for_prompt;

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Handle a `UserPromptSubmit` hook.
    ///
    /// The first hook for a given `session_id` registers that session
    /// (SessionStart never fires); routing by id lets several Claude Code
    /// sessions register independently — each lands on its own actor.
    ///
    /// Correlation is against the ONE outstanding send: under the
    /// single-outstanding dispatch rule at most one `dispatched` send exists
    /// per session, so the hook's prompt either equals that send's text (the
    /// echo of Delta's own dispatch — resolved *before* syncing, so the
    /// locator quote is returned as `additionalContext` even when the user's
    /// transcript line has not been written yet) or it is external input. The
    /// text equality is kept as a sanity check: a mismatch means the
    /// keystrokes were mangled (e.g. interleaved pane typing), so it is logged
    /// loudly, the prompt is treated as the external input it textually is,
    /// and the turn machine returns the outstanding send to `queued` so it
    /// re-dispatches intact once this turn ends.
    ///
    /// The actual message→thread attribution (and `mark_send_matched`) happens
    /// inside [`Self::sync_transcript`], keyed by comparing each ingested user
    /// line against the outstanding send. A [`SessionEvent::TurnStarted`] is
    /// emitted when the user line for this prompt was attributed in this sync;
    /// otherwise the later `TurnCompleted` triggers the UI refetch.
    /// [`SessionEvent::ExternalInput`] is emitted only when the prompt matched
    /// no outstanding send — except for harness-injected task-notification
    /// prompts, which also match no send but are not pane typing, so they are
    /// suppressed.
    ///
    /// Returns the events to broadcast and, when a locator quote should be
    /// injected, the `additionalContext` string for the hook response.
    pub(in crate::interactor) async fn on_user_prompt_submit(
        &mut self,
        hook: UserPromptSubmitHook,
    ) -> Result<(Vec<SessionEvent>, Option<String>)> {
        let mut events = Vec::new();

        // Bind a pending Delta spawn for THIS session id, if one is waiting: the
        // spawn's session row was created eagerly (status `spawning`) when the
        // id was minted, so its existence cannot signal "already contacted" —
        // the runtime bind is what distinguishes first contact. The bind is
        // idempotent and cheap, so it runs on every hook; when nothing was
        // pending, fall back to the stored row, registering an external
        // session on its first contact.
        let session = match self
            .bind_pending_spawn(&hook.cwd, &hook.transcript_path, &mut events)
            .await?
        {
            Some(session) => session,
            None => match self.store.session(&hook.session_id).await? {
                Some(session) => session,
                None => self.register_on_first_contact(&hook, &mut events).await?,
            },
        };

        // Resolve this prompt against the one outstanding send *before*
        // syncing, so the locator quote is returned as `additionalContext`
        // even when the user line has not been ingested yet (the common
        // timing case).
        let outstanding = self.store.head_dispatched_send(&hook.session_id).await?;
        let pending = outstanding
            .as_ref()
            .filter(|send| send.text.trim() == hook.prompt.trim())
            .cloned();
        // Resolve the `additionalContext` note *before* syncing, so the current
        // user line is not yet ingested and `latest_user_thread` still reports
        // the PREVIOUS thread the user was in — letting us detect a switch.
        let additional_context = self
            .thread_switch_context(&hook.session_id, pending.as_ref())
            .await?;

        // Ingest new transcript lines. This compares each user line against
        // the outstanding send and attributes it (plus the assistant lines
        // that follow it) to the right thread, marking the send matched as a
        // side effect. Any permission-resolution events the ingest produced
        // are broadcast too.
        let (new_messages, resolved_events) = self.sync_transcript(&session).await?;
        events.extend(resolved_events);

        // A prompt submission means a turn is now in flight — whether Delta
        // dispatched it or it was typed straight into the embedded pane. Feed
        // the turn machine AFTER the sync, so an interrupt marker of the
        // *previous* turn in the same batch is consumed first. The machine
        // moves to `InFlight` either way; on the mismatch path (an outstanding
        // send exists but the prompt is not its echo) the transition also
        // returns that send to `queued` and is logged loudly.
        match pending {
            Some(pending) => {
                self.apply_turn_input(TurnInput::EchoMatched {
                    send_id: pending.id,
                })
                .await?;
                // The outstanding send matches this prompt. If its user line
                // was attributed in this very sync, announce the turn now;
                // otherwise the line was not in the JSONL yet (the common
                // timing case) and the later `Stop` sync attributes it, with
                // `TurnCompleted` driving the UI refetch.
                if let Some(uuid) = match_uuid_for_prompt(&new_messages, &hook.prompt) {
                    events.push(SessionEvent::TurnStarted {
                        session_id: hook.session_id.clone(),
                        send_id: pending.id,
                        // The dispatched send carries the thread it was composed
                        // for, so the running indicator lights on that exact
                        // thread (main or a branch) rather than the session.
                        thread_id: pending.thread_id,
                        matched_uuid: uuid,
                    });
                }
            }
            None => {
                if let Some(outstanding) = &outstanding {
                    tracing::warn!(
                        session_id = %hook.session_id,
                        send_id = outstanding.id,
                        expected = %outstanding.text.trim(),
                        got = %hook.prompt.trim(),
                        "UserPromptSubmit does not echo the outstanding send: the \
                         dispatched keystrokes were mangled or overtaken; treating the \
                         prompt as external input (the turn machine requeues the send)"
                    );
                }
                self.apply_turn_input(TurnInput::ExternalPrompt).await?;
                // No outstanding send matched this prompt. A background-task
                // notification is injected by the harness as a prompt
                // submission, not typed into the pane, so it must not surface as
                // external input.
                if !claude_format::is_task_notification(&hook.prompt) {
                    events.push(SessionEvent::ExternalInput {
                        session_id: hook.session_id.clone(),
                        prompt: hook.prompt.clone(),
                    });
                }
            }
        }

        Ok((events, additional_context))
    }
}
