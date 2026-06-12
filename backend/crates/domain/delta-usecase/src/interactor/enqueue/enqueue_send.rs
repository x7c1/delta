use delta_model::Send;

use crate::error::{Error, Result};
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::send_target::SendTarget;
use crate::Interactor;

use super::new_session_placeholder_send;

impl<T, X, S, W> Interactor<T, X, S, W>
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
    ///   is spawned with the text held as its `first_prompt`. The
    ///   `send` row cannot be written yet (it references a session id
    ///   that does not exist), so it is held on the spawn and written when the
    ///   first `UserPromptSubmit` binds the spawn. A synthetic, not-yet-persisted
    ///   [`Send`] is returned so the REST surface has a response, carrying
    ///   the still-unknown target thread as `0` (the real id is assigned at bind
    ///   time on the new session's `main`).
    ///
    /// A branch send (the `branch_from` arm of [`SendTarget::Thread`]) requires
    /// an existing session — there must be a message to branch from — which the
    /// thread target inherently provides.
    pub async fn enqueue_send(
        &self,
        target: SendTarget,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
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
                // No session yet: spawn one with the text held as its first
                // prompt, in the user-selected `workdir` when given (validated by
                // `spawn_fresh` before any pane is created) or the default
                // per-spawn directory otherwise. The real `send` row is
                // written when the first `UserPromptSubmit` binds the spawn.
                //
                // `locator_quote` is intentionally dropped here, not forwarded to
                // the spawn: a brand-new session has no earlier passage to anchor,
                // so there is nothing to locate. It is still echoed in the
                // synthetic response below as a courtesy to the caller, but the
                // held first prompt (and the row written at bind time) carry
                // no quote.
                self.spawn_fresh(Some(text.to_owned()), workdir).await?;
                Ok(new_session_placeholder_send(text, locator_quote))
            }
        }
    }
}
