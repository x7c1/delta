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
    /// ## Which send this prompt belongs to is decided by POSITION
    ///
    /// `UserPromptSubmit` carries no id Delta could round-trip, only text — and
    /// Claude Code freely rewrites a prompt between the keystrokes landing and
    /// the submission (local-command folding, the unknown-command notice,
    /// namespace expansion, the `[Image #N]` prefix), so text equality cannot
    /// answer "did my send's turn start?". Position can: under the
    /// single-outstanding dispatch rule at most one `dispatched` send exists
    /// per session, and while it is outstanding its keystrokes are already in
    /// the pane — so a prompt arriving now is *that send's*, whatever it says.
    /// The handler therefore feeds the turn machine
    /// [`TurnInput::EchoMatched`] whenever a send is outstanding, and
    /// [`TurnInput::ExternalPrompt`] only when there is none — or when the
    /// session is still inside its resume window, where the send's keystrokes
    /// are deliberately *held* and so cannot be what submitted.
    ///
    /// The trade is deliberate: a prompt genuinely typed into the pane while a
    /// Delta send is outstanding is credited to that send. Worst case the
    /// message is delivered once and filed under the wrong thread — against the
    /// old behaviour's worst case of delivering it twice (a rewritten echo
    /// requeued the send, so the same text was typed again, one model turn per
    /// attempt) or wedging it as permanently "In Progress".
    ///
    /// ## Which thread its lines belong to is still decided by TEXT
    ///
    /// Attribution keeps using [`claude_format::prompt_echoes_send`] (exact
    /// equality for a plain send, widened to absorb the image-attachment
    /// rewrite): only a textual match resolves the send *this* prompt is,
    /// which is what supplies the `additionalContext` locator quote and the
    /// [`SessionEvent::TurnStarted`] announcement, and — inside
    /// [`Self::sync_transcript`], by comparing ingested user lines — the
    /// `mark_send_matched` that binds the send to a transcript uuid. A
    /// consumed-but-unattributed send is settled as delivered when its turn
    /// ends (`OrphanedSend::SettleIfUnmatched`), so the row never lingers
    /// `dispatched` and is never reported as failed.
    ///
    /// [`SessionEvent::ExternalInput`] follows consumption, not text: it is
    /// emitted only when NO send was consumed. Announcing a rewritten echo as
    /// pane typing would contradict the very decision the turn machine just
    /// made — and would put a spurious "typed in the pane" notice on Delta's
    /// own message. Harness-injected task-notification prompts consume no send
    /// either, but are not pane typing, so they stay suppressed.
    ///
    /// Returns the events to broadcast and, when a locator quote should be
    /// injected, the `additionalContext` string for the hook response.
    ///
    /// [`SessionContext::apply_turn_input`]: Self::apply_turn_input
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
        // CONSUMPTION, by position: an outstanding send whose keystrokes really
        // are in the pane owns this prompt. Inside the resume window they are
        // held instead of typed, so nothing is consumed there.
        let consumed = outstanding
            .as_ref()
            .filter(|_| !self.state.is_resuming())
            .cloned();
        // ATTRIBUTION, by text: only a prompt that still reads as the send's
        // own text resolves *which* send it is, which is what the locator-quote
        // note and the turn announcement are keyed on.
        let attributed = outstanding
            .as_ref()
            .filter(|send| claude_format::prompt_echoes_send(&send.text, &hook.prompt))
            .cloned();
        // Resolve the `additionalContext` note *before* syncing, so the current
        // user line is not yet ingested and `latest_user_thread` still reports
        // the PREVIOUS thread the user was in — letting us detect a switch.
        let additional_context = self
            .thread_switch_context(&hook.session_id, attributed.as_ref())
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
        // moves to `InFlight` either way; which send (if any) it credits with
        // the turn is the positional decision made above.
        match consumed {
            Some(consumed) => {
                if attributed.is_none() {
                    tracing::info!(
                        session_id = %hook.session_id,
                        send_id = consumed.id,
                        expected = %consumed.text.trim(),
                        got = %hook.prompt.trim(),
                        "UserPromptSubmit does not equal the outstanding send's text \
                         (Claude Code rewrote the prompt, most likely): the send is the \
                         only thing that could have submitted here, so its turn starts \
                         and it is NOT re-typed; only its thread attribution is left to \
                         the transcript text"
                    );
                }
                self.apply_turn_input(TurnInput::EchoMatched {
                    send_id: consumed.id,
                })
                .await?;
                // Announce the turn only when the text also resolved the send:
                // `TurnStarted` carries the matched uuid and the thread to
                // light up, both of them attribution facts. If the send's user
                // line was attributed in this very sync, announce now;
                // otherwise the line was not in the JSONL yet (the common
                // timing case) and the later `Stop` sync attributes it, with
                // `TurnCompleted` driving the UI refetch.
                if let Some(attributed) = &attributed {
                    if let Some(uuid) = match_uuid_for_prompt(&new_messages, &hook.prompt) {
                        events.push(SessionEvent::TurnStarted {
                            session_id: hook.session_id.clone(),
                            send_id: attributed.id,
                            // The dispatched send carries the thread it was
                            // composed for, so the running indicator lights on
                            // that exact thread (main or a branch) rather than
                            // the session.
                            thread_id: attributed.thread_id,
                            matched_uuid: uuid,
                        });
                    }
                }
            }
            None => {
                if let Some(outstanding) = &outstanding {
                    tracing::debug!(
                        session_id = %hook.session_id,
                        send_id = outstanding.id,
                        "UserPromptSubmit arrived while a send is outstanding but its \
                         keystrokes are still held for the resume window; the prompt \
                         cannot be that send's, so it is external input"
                    );
                }
                self.apply_turn_input(TurnInput::ExternalPrompt).await?;
                // This prompt consumed no send, so it really is input from
                // outside Delta — except a background-task notification, which
                // the harness injects as a prompt submission rather than typing
                // it into the pane, so it must not surface as external input.
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
