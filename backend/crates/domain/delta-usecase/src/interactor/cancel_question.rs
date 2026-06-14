//! Cancelling a pending `AskUserQuestion` from the browser by injecting the
//! `Escape` keystroke into the session's live TUI pane.
//!
//! A CLI hook cannot cancel the question, so when the browser cancels a question
//! card, Delta does what a human would at the keyboard: it presses `Escape` in
//! the pane. A single `Escape` cancels the whole call (the pinned, unit-tested
//! [`cancel_keys`] generator returns `[Escape]` for any call shape). The TUI
//! then writes a `tool_result` with `is_error: true` for the question's
//! `tool_use_id`, and that flush resolves the question's request row through the
//! normal sync — so the card clears via the existing `PermissionResolved` path
//! with no new clear logic here, exactly as an answered (or terminal-cancelled)
//! question does.
//!
//! This is the sibling of [`answer_question`](super::answer_question): same
//! `request_id` correlation against the pending question, same "no pending /
//! stale / no live pane → fall back to the terminal" handling, same single
//! `send_keys` injection — it differs only in the key sequence (a lone `Escape`)
//! and in needing no selection payload.

use crate::error::{Error, Result};
use crate::interactor::question_keys::cancel_keys;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W> SessionContext<'_, T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Cancel the session's pending `AskUserQuestion` by injecting `Escape` into
    /// the live TUI pane.
    ///
    /// Correlated by `request_id` against the recorded
    /// [`pending_question`](crate::interactor::session_actor::runtime::SessionRuntime),
    /// exactly like [`answer_question`](Self::answer_question):
    ///
    /// - No pending question, one with a different id, or no live pane →
    ///   [`Error::QuestionNotPending`] (`409`), a graceful no-op the browser
    ///   surfaces as the cancel-in-the-terminal fallback.
    ///
    /// There is no malformed-input case here (cancel carries no selection), so
    /// unlike `answer_question` this never returns [`Error::InvalidQuestionAnswer`].
    ///
    /// On success the `Escape` is injected and `Ok(())` returned. The pending
    /// question is **not** cleared here: the authoritative clear is the
    /// `is_error` `tool_result` ingest resolving the request row (the same path
    /// an answered question takes), keeping one clear path. A dispatch failure
    /// propagates so the caller can report it and the user can retry in the
    /// terminal.
    pub(in crate::interactor) async fn cancel_question(&mut self, request_id: i64) -> Result<()> {
        // The question must still be the one pending: a keyed match guards
        // against a stale cancel landing after the question already resolved (or
        // a newer one replaced it).
        let Some(pending) = self.state.pending_question() else {
            return Err(Error::QuestionNotPending(request_id));
        };
        if pending.request_id != request_id {
            return Err(Error::QuestionNotPending(request_id));
        }

        // No live pane means the TUI is gone; treat it like a stale question so
        // the browser falls back to the terminal rather than reporting a server
        // fault.
        let Some(pane) = self.state.handle().map(|handle| handle.pane.clone()) else {
            return Err(Error::QuestionNotPending(request_id));
        };

        let keys = cancel_keys();
        let names: Vec<&str> = keys.iter().map(|key| key.tmux_name()).collect();
        self.tmux.send_keys(&pane, &names).await?;
        Ok(())
    }
}
