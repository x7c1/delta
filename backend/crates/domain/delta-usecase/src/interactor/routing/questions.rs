//! Question answers: answering or cancelling a session's pending
//! `AskUserQuestion`. Unlike a permission decision, both carry the session id
//! in their URL, so they route straight to the owning actor.

use delta_model::SessionId;

use crate::error::Result;
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Answer a session's pending `AskUserQuestion` by injecting the selection
    /// keystrokes into its live TUI pane.
    ///
    /// Unlike a permission decision (keyed only by request id, so it needs the
    /// id→session index), a question answer carries the session id in its URL,
    /// so it routes straight to the owning actor. The actor correlates the
    /// `request_id` against its pending question, builds the pinned key sequence,
    /// and injects it via the tmux driver. `selections[q]` holds the chosen
    /// 0-based option indices for question `q`.
    ///
    /// Returns [`Error::QuestionNotPending`] (`409`) when no matching question
    /// is pending (already answered, stale, or no live pane), and
    /// [`Error::InvalidQuestionAnswer`] (`400`) for a malformed selection — in
    /// both cases the browser falls back to the terminal.
    ///
    /// [`Error::QuestionNotPending`]: crate::error::Error::QuestionNotPending
    /// [`Error::InvalidQuestionAnswer`]: crate::error::Error::InvalidQuestionAnswer
    pub async fn answer_question(
        &self,
        session_id: &SessionId,
        request_id: i64,
        selections: Vec<Vec<usize>>,
    ) -> Result<()> {
        self.request(session_id, |reply| SessionInput::AnswerQuestion {
            request_id,
            selections,
            reply,
        })
        .await
    }

    /// Cancel a session's pending `AskUserQuestion` by injecting `Escape` into
    /// its live TUI pane (a single Escape cancels the whole call).
    ///
    /// The sibling of [`answer_question`](Self::answer_question): like an answer,
    /// it carries the session id in its URL so it routes straight to the owning
    /// actor, which correlates the `request_id` against its pending question and
    /// injects the cancel keystroke via the tmux driver.
    ///
    /// Returns [`Error::QuestionNotPending`] (`409`) when no matching question is
    /// pending (already answered/cancelled, stale, or no live pane), in which
    /// case the browser falls back to the terminal. Unlike an answer there is no
    /// `400` case — cancel carries no selection to malform.
    ///
    /// [`Error::QuestionNotPending`]: crate::error::Error::QuestionNotPending
    pub async fn cancel_question(&self, session_id: &SessionId, request_id: i64) -> Result<()> {
        self.request(session_id, |reply| SessionInput::CancelQuestion {
            request_id,
            reply,
        })
        .await
    }
}
