use crate::error::Result;
use crate::ports::{
    SessionEvent, SessionStore, TmuxDriver, Transcript, UserPromptSubmitHook, Workspace,
};
use crate::Interactor;

use super::match_uuid_for_prompt;

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
    /// is emitted only when no queued send matched this prompt at all.
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
                // No queued send matched this prompt at all: external input.
                events.push(SessionEvent::ExternalInput {
                    session_id: hook.session_id.clone(),
                    prompt: hook.prompt.clone(),
                });
            }
        }

        Ok((events, additional_context))
    }
}
