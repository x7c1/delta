use delta_model::{MessageUuid, Send, ThreadId};

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W> SessionContext<'_, T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Enqueue a user input to a thread of this session.
    ///
    /// The routing layer already derived the owning session from the target
    /// thread (a stale or wrong id surfaced as a clean `ThreadNotFound` there,
    /// before reaching this actor). Here the session is ensured open — resumed
    /// via [`Self::open_session`] (`claude --resume <id>`) when it is known
    /// but closed — and then the normal pre-dispatch path runs: the `send`
    /// row is written *before* the keystrokes, so the correlation head is in
    /// place when the `UserPromptSubmit` hook fires, with the
    /// cancel-on-dispatch-failure rollback.
    ///
    /// A branch send (`branch_from: Some`) requires an existing session —
    /// there must be a message to branch from — which the thread target
    /// inherently provides.
    ///
    /// Returns the created send plus any [`SessionEvent`]s the enqueue
    /// produced (e.g. a `send_dispatched` when the idle-flush promoted a
    /// previously queued send); the transport broadcasts them.
    pub(in crate::interactor) async fn enqueue_to_thread(
        &mut self,
        thread_id: ThreadId,
        branch_from: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<(Send, Vec<SessionEvent>)> {
        // Ensure the session is open: resume it if it is known but closed (no
        // live pane). Once open we have a pane to dispatch to and the normal
        // pre-dispatch path applies.
        let pane = self.ensure_open().await?;
        self.enqueue_into_open(&pane, thread_id, text, locator_quote, branch_from)
            .await
    }
}
