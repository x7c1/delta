use delta_model::{Send, SessionId};

use crate::error::Result;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::interactor::InteractorCore;

use super::{frame_branch_entry_context, frame_locator_context, frame_thread_switch_context};

impl<T, X, S, W> InteractorCore<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Compute the `additionalContext` note to inject for this prompt.
    ///
    /// Branching in Delta is view-only: the model only ever sees the single
    /// linear `parentUuid` chain, never Delta's thread tree. So when the user
    /// moves to a different thread and keeps talking, the model has no signal
    /// that the topic changed and may misread an utterance like "is that what
    /// you mean?" as referring to the (unrelated) message immediately above.
    /// This produces a short natural-language note that gives the model that
    /// missing signal.
    ///
    /// Must be called *before* `sync_transcript`, so the current user line is
    /// not yet ingested and [`SessionStore::latest_user_thread`] still reports
    /// the PREVIOUS thread the user was in. Four cases:
    ///
    /// 1. No queued send matched this prompt → external input → inject nothing.
    /// 2. The send carries a locator quote → first entry into a branch → keep
    ///    the locator-quote frame and bind it to the target thread.
    /// 3. No locator and the previous thread is KNOWN and differs from the
    ///    target → a thread switch / re-visit → inject a re-focus note (with the
    ///    target thread's root quote, unless it is `main`).
    /// 4. No locator and either the target thread is unchanged or the previous
    ///    thread is unknown (first turn / first prompt after a resume) → not a
    ///    switch → inject nothing.
    pub(in crate::interactor) async fn thread_switch_context(
        &self,
        session_id: &SessionId,
        pending: Option<&Send>,
    ) -> Result<Option<String>> {
        // Case 1: external input — no queued send to attribute → inject nothing.
        let Some(pending) = pending else {
            return Ok(None);
        };
        let cur = pending.thread_id;

        // Case 2: first entry into a branch — the user selected a passage to
        // anchor this message. Keep the locator-quote frame and tell the model
        // that this quote roots the thread it is now in.
        if let Some(quote) = pending.locator_quote.as_deref() {
            if let Some(frame) = frame_locator_context(quote) {
                return Ok(Some(frame_branch_entry_context(&frame, cur)));
            }
        }

        // Cases 3 & 4 hinge on whether the active thread changed. `prev` is the
        // thread of the latest already-persisted user line (this prompt's line
        // is not synced yet), i.e. the thread the user was in before this send.
        //
        // Only a KNOWN switch warrants a re-focus note. `prev == None` means the
        // previous thread is unknown — there is no persisted user line yet. That
        // happens on the very first turn and, crucially, on the first prompt
        // after a session resume (the prior turn's user line is not visible to
        // `latest_user_thread` at the resume boundary, since this runs before
        // `sync_transcript`). Asserting a switch there is false: injecting a
        // "switched to thread:N" note misleads the model into treating an
        // ordinary continuation as a re-visit to an earlier discussion. So a
        // switch is asserted only when `prev` is known and differs from `cur`
        // (Case 4 — same/unknown thread — falls through to no injection).
        let prev = self.store.latest_user_thread(session_id).await?;
        let Some(prev) = prev.filter(|p| *p != cur) else {
            return Ok(None);
        };

        // Case 3: thread switch / re-visit. Cite the target thread's root quote
        // so the re-focus survives even if the original binding scrolled out of
        // context. Only branch threads (those with a parent) have a root quote;
        // `main` has none, so it is cited by name only.
        let root_quote = match self.store.thread(cur).await? {
            Some(thread) => thread.parent_thread_id.is_some().then_some(thread.title),
            None => None,
        };
        Ok(Some(frame_thread_switch_context(
            prev,
            cur,
            root_quote.as_deref(),
        )))
    }
}
