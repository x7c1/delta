use crate::error::Result;
use crate::ports::{
    SessionEvent, SessionStore, TmuxDriver, Transcript, UserPromptSubmitHook, Workspace,
};
use crate::Interactor;

use super::match_uuid_for_prompt;

/// Prompt prefix Claude Code uses when it injects a background-task
/// completion notification. Such a submission is a harness injection, not a
/// human typing into the pane, so it must not be reported as external input.
const TASK_NOTIFICATION_PREFIX: &str = "<task-notification>";

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Handle a `UserPromptSubmit` hook.
    ///
    /// The first hook for a given `session_id` registers that session
    /// (SessionStart never fires); routing by id lets several Claude Code
    /// sessions register independently.
    ///
    /// The locator quote to inject as `additionalContext` is resolved *before*
    /// syncing, by matching the prompt text against the queued `pending_send`
    /// (by text, not FIFO position). This is timing-independent: the quote is
    /// returned even when the user's transcript line has not been written yet.
    ///
    /// The actual message→thread attribution (and `mark_send_matched`) happens
    /// inside [`Self::sync_transcript`], keyed by matching each ingested user
    /// line to its queued send. A [`SessionEvent::TurnStarted`] is emitted when
    /// the user line for this prompt was attributed in this sync; otherwise the
    /// later `TurnCompleted` triggers the UI refetch. [`SessionEvent::ExternalInput`]
    /// is emitted only when no queued send matched this prompt at all — except
    /// for harness-injected task-notification prompts, which also match no send
    /// but are not pane typing, so they are suppressed.
    ///
    /// Returns the events to broadcast and, when a locator quote should be
    /// injected, the `additionalContext` string for the hook response.
    pub async fn on_user_prompt_submit(
        &self,
        hook: UserPromptSubmitHook,
    ) -> Result<(Vec<SessionEvent>, Option<String>)> {
        let mut events = Vec::new();

        // Register on first contact for THIS session id (Claude Code never fires
        // SessionStart). Routing by id lets several Claude Code sessions register
        // independently rather than assuming a single global one.
        let session = match self.store.session(&hook.session_id).await? {
            Some(session) => session,
            None => self.register_on_first_contact(&hook, &mut events).await?,
        };

        // Resolve this prompt's queued send *before* syncing, so the locator
        // quote is returned as `additionalContext` even when the user line has
        // not been ingested yet (the common timing case). Match by text — not by
        // FIFO head — so a stale send stuck at the head cannot suppress the quote
        // or misfire external-input detection.
        let pending = self
            .store
            .match_pending_send(&hook.session_id, hook.prompt.trim())
            .await?;
        // Resolve the `additionalContext` note *before* syncing, so the current
        // user line is not yet ingested and `latest_user_thread` still reports
        // the PREVIOUS thread the user was in — letting us detect a switch.
        let additional_context = self
            .thread_switch_context(&hook.session_id, pending.as_ref())
            .await?;

        // Ingest new transcript lines. This matches each user line to its queued
        // send and attributes it (plus the assistant lines that follow it) to
        // the right thread, marking the send matched as a side effect. Any
        // permission-resolution events the ingest produced are broadcast too.
        let (new_messages, resolved_events) = self.sync_transcript(&session).await?;
        events.extend(resolved_events);

        // A prompt submission means a turn is now in flight — whether Delta
        // dispatched it or it was typed straight into the embedded pane. Mark it
        // so a branch/quoted send arriving mid-turn defers instead of dispatching
        // into a busy session. Delta's own dispatch already sets this; doing it
        // here also covers pane-started turns Delta did not dispatch. Cleared on
        // `Stop` (see `on_stop`) or interrupt (see `sync_transcript`).
        self.store.set_turn_active(&hook.session_id, true).await?;

        match pending {
            Some(pending) => {
                // A queued send matches this prompt. If its user line was
                // attributed in this very sync, announce the turn now; otherwise
                // the line was not in the JSONL yet (the common timing case) and
                // the later `Stop` sync attributes it, with `TurnCompleted`
                // driving the UI refetch.
                if let Some(uuid) = match_uuid_for_prompt(&new_messages, &hook.prompt) {
                    events.push(SessionEvent::TurnStarted {
                        session_id: hook.session_id.clone(),
                        pending_send_id: pending.id,
                        matched_uuid: uuid,
                    });
                }
            }
            None => {
                // No queued send matched this prompt. A background-task
                // notification is injected by the harness as a prompt
                // submission, not typed into the pane, so it must not surface as
                // external input.
                if !hook.prompt.trim_start().starts_with(TASK_NOTIFICATION_PREFIX) {
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
