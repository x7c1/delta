//! The enqueue: a user's text routed to the session its target names — the
//! session that owns an existing thread, or a fresh one minted for a
//! composer-first message. The send lifecycle's entry point, kept apart from
//! the lifecycle commands because it derives its session from the target
//! rather than acting on one it is handed; its later steps (`cancel_send`,
//! `release_send`) live in `send_cancellation`.

use delta_model::Send;

use crate::error::{Error, Result};
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::send_target::SendTarget;

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Enqueue a user input, routing it to the session the target names.
    ///
    /// The session is determined by the [`SendTarget`], never by a global
    /// "current" session:
    ///
    /// - [`SendTarget::Thread`] — an existing conversation. The owning session
    ///   is derived from the thread here (a read), then the enqueue executes
    ///   on that session's actor: ensured open (resumed if closed), the `send`
    ///   row written before the keystrokes, deferred `queued` when a turn is
    ///   in flight.
    /// - [`SendTarget::NewSession`] — a composer-first message. A fresh
    ///   session id is minted, its actor spawned, and the launch executes
    ///   there: the session row (status `spawning`), its `main` thread, and
    ///   the `send` row are all written *before* `claude` launches, so the
    ///   returned [`Send`] carries real ids.
    ///
    /// Returns the created send plus any [`SessionEvent`]s the enqueue
    /// produced; the transport broadcasts them.
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
                // wrong id becomes a clean `ThreadNotFound` (404) rather than
                // an opaque failure downstream.
                let thread = self
                    .store
                    .thread(thread_id)
                    .await?
                    .ok_or_else(|| Error::ThreadNotFound(thread_id.value()))?;
                let session_id = thread.session_id;
                let text = text.to_owned();
                let locator_quote = locator_quote.map(str::to_owned);
                self.request(&session_id, move |reply| SessionInput::EnqueueToThread {
                    thread_id,
                    branch_from,
                    text,
                    locator_quote,
                    reply,
                })
                .await
            }
            SendTarget::NewSession {
                workdir,
                launch_option_ids,
                worktree,
                provider,
                pull_request_number,
            } => {
                // `locator_quote` is intentionally dropped here, not forwarded
                // to the spawn: a brand-new session has no earlier passage to
                // anchor, so there is nothing to locate. The persisted row
                // (and therefore the response) carries no quote.
                let id = Self::mint_session_id();
                let text = text.to_owned();
                let spawn = self
                    .request(&id, move |reply| SessionInput::SpawnFresh {
                        first_prompt: Some(text),
                        workdir,
                        launch_option_ids,
                        worktree,
                        provider,
                        pull_request_number,
                        reply,
                    })
                    .await?;
                let send = spawn
                    .first_send
                    .expect("spawn_fresh enqueues a send when a first prompt is given");
                Ok((send, Vec::new()))
            }
        }
    }
}
