use delta_model::Send;

use crate::error::{Error, Result};
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::send_target::SendTarget;
use crate::interactor::InteractorCore;

impl<T, X, S, W> InteractorCore<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Enqueue a user input, routing it to the session the target names.
    ///
    /// The session is determined by the [`SendTarget`], never by a global
    /// "current" session:
    ///
    /// - [`SendTarget::Thread`] — an existing conversation. The session is
    ///   derived from the thread (threads belong to a session), then ensured
    ///   open before the text is dispatched:
    ///   - **Open** (a live pane is bound): the text is dispatched immediately on
    ///     the normal path — the `send` row is written *before* the
    ///     keystrokes, so the correlation head is in place when the
    ///     `UserPromptSubmit` hook fires, with the cancel-on-dispatch-failure
    ///     rollback below.
    ///   - **Closed** (the session exists in the store but no live pane): it is
    ///     resumed via [`Self::open_session`] (`claude --resume <id>`), then the
    ///     normal path runs.
    /// - [`SendTarget::NewSession`] — a composer-first message. A fresh session
    ///   is spawned with the text delivered as its launch-time first prompt.
    ///   Delta mints the session id before launching, so the session row
    ///   (status `spawning`), its `main` thread, and the `send` row are all
    ///   written *before* the spawn — the returned [`Send`] carries the real
    ///   session/thread/send ids, and the first `UserPromptSubmit` correlates
    ///   through the normal FIFO machinery.
    ///
    /// A branch send (the `branch_from` arm of [`SendTarget::Thread`]) requires
    /// an existing session — there must be a message to branch from — which the
    /// thread target inherently provides.
    ///
    /// Returns the created send plus any [`SessionEvent`]s the enqueue produced
    /// (e.g. a `send_dispatched` when the idle-flush promoted a previously
    /// queued send); the transport broadcasts them.
    pub async fn enqueue_send(
        &self,
        target: SendTarget,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<(Send, Vec<SessionEvent>)> {
        match target {
            SendTarget::Thread {
                thread_id,
                branch_from,
            } => {
                // Derive the owning session from the target thread. A stale or
                // wrong id becomes a clean `ThreadNotFound` (404) rather than an
                // opaque failure downstream.
                let thread = self
                    .store
                    .thread(thread_id)
                    .await?
                    .ok_or_else(|| Error::ThreadNotFound(thread_id.value()))?;
                let session_id = thread.session_id;
                // Ensure the session is open: resume it if it is known but closed
                // (no live pane). Once open we have a pane to dispatch to and the
                // normal pre-dispatch path applies.
                let pane = self.ensure_open(&session_id).await?;
                self.enqueue_into_open(
                    &session_id,
                    &pane,
                    thread_id,
                    text,
                    locator_quote,
                    branch_from.as_ref(),
                )
                .await
            }
            SendTarget::NewSession { workdir } => {
                // Spawn a fresh session with the text as its launch-time first
                // prompt, in the user-selected `workdir` when given (validated
                // by `spawn_fresh` before any row or pane is created) or the
                // default per-spawn directory otherwise. `spawn_fresh` writes
                // the session row, its `main` thread, and the send row before
                // launching, so the returned send carries real ids.
                //
                // `locator_quote` is intentionally dropped here, not forwarded
                // to the spawn: a brand-new session has no earlier passage to
                // anchor, so there is nothing to locate. The persisted row (and
                // therefore the response) carries no quote.
                let spawn = self.spawn_fresh(Some(text.to_owned()), workdir).await?;
                let send = spawn
                    .first_send
                    .expect("spawn_fresh enqueues a send when a first prompt is given");
                Ok((send, Vec::new()))
            }
        }
    }
}
