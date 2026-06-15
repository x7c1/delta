//! Answering a pending `AskUserQuestion` from the browser by injecting the
//! selection keystrokes into the session's live TUI pane.
//!
//! A CLI hook cannot return the user's pick, so when the browser answers a
//! question card, Delta drives `claude`'s on-screen `AskUserQuestion` widget the
//! way a human would: it builds the exact key sequence (the pinned, unit-tested
//! [`answer_keys`] generator) and injects it into the pane via the tmux driver's
//! [`send_keys`](crate::ports::TmuxDriver::send_keys). The TUI then records the
//! answer, the turn proceeds, and the eventual `tool_result` resolves the
//! question's request row through the normal sync — so the card clears via the
//! existing `PermissionResolved` path with no new clear logic here.
//!
//! The injection is inherently coupled to the TUI's layout (pinned for claude
//! v2.1.177); the question card keeps an "Open terminal" fallback so a misfire
//! never strands the user.

use crate::error::{Error, Result};
use crate::interactor::question_keys::{answer_keys, parse_question_shapes, QuestionKeyError};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Answer the session's pending `AskUserQuestion` by injecting the keystrokes
    /// for `selections` (the chosen 0-based option indices per question) into the
    /// live TUI pane.
    ///
    /// Correlated by `request_id` against the recorded
    /// [`pending_question`](crate::interactor::session_actor::runtime::SessionRuntime):
    ///
    /// - No pending question, or one with a different id (already answered, stale,
    ///   or a newer question replaced it) → [`Error::QuestionNotPending`]
    ///   (`409`), a graceful no-op the browser surfaces as the
    ///   answer-in-the-terminal fallback.
    /// - A malformed selection, or a sub-case the generator refuses to drive
    ///   (multi-select within a multi-question call) → [`Error::InvalidQuestionAnswer`]
    ///   (`400`).
    /// - No live pane (the session closed under us) → [`Error::QuestionNotPending`]:
    ///   there is no TUI to answer, so the UI falls back exactly as for a stale
    ///   question.
    ///
    /// On success the keys are injected and `Ok(())` returned. The pending
    /// question is **not** cleared here: the authoritative clear is the
    /// `tool_result` ingest resolving the request row (the same path a
    /// terminal-answered question takes), keeping one clear path. A dispatch
    /// failure propagates so the caller can report it and the user can retry in
    /// the terminal.
    pub(in crate::interactor) async fn answer_question(
        &mut self,
        request_id: i64,
        selections: &[Vec<usize>],
    ) -> Result<()> {
        // The question must still be the one pending: a keyed match guards
        // against a stale answer landing after the question already resolved
        // (or a newer one replaced it).
        let Some(pending) = self.state.pending_question() else {
            return Err(Error::QuestionNotPending(request_id));
        };
        if pending.request_id != request_id {
            return Err(Error::QuestionNotPending(request_id));
        }

        let shapes = parse_question_shapes(&pending.tool_input_json).ok_or_else(|| {
            Error::InvalidQuestionAnswer(format!(
                "pending question {request_id} has no parseable options"
            ))
        })?;
        let keys = answer_keys(&shapes, selections)
            .map_err(|err| Error::InvalidQuestionAnswer(describe_key_error(request_id, &err)))?;

        // No live pane means the TUI is gone; treat it like a stale question so
        // the browser falls back to the terminal rather than reporting a server
        // fault.
        let Some(pane) = self.state.handle().map(|handle| handle.pane.clone()) else {
            return Err(Error::QuestionNotPending(request_id));
        };

        let names: Vec<&str> = keys.iter().map(|key| key.tmux_name()).collect();
        self.tmux.send_keys(&pane, &names).await?;
        Ok(())
    }
}

/// Render a [`QuestionKeyError`] as the message carried by
/// [`Error::InvalidQuestionAnswer`].
fn describe_key_error(request_id: i64, err: &QuestionKeyError) -> String {
    match err {
        QuestionKeyError::NoQuestions => {
            format!("question {request_id} carries no questions")
        }
        QuestionKeyError::SelectionCountMismatch {
            questions,
            selections,
        } => format!(
            "question {request_id} expects {questions} selection group(s), got {selections}"
        ),
        QuestionKeyError::OptionOutOfRange {
            question,
            option,
            option_count,
        } => format!(
            "question {request_id}: option {option} is out of range for sub-question \
             {question} ({option_count} option(s))"
        ),
        QuestionKeyError::SingleSelectNeedsOneOption { question, selected } => format!(
            "question {request_id}: single-select sub-question {question} needs exactly \
             one option, got {selected}"
        ),
        QuestionKeyError::MultiSelectNeedsSelection { question } => format!(
            "question {request_id}: multi-select sub-question {question} needs at least \
             one option"
        ),
    }
}
