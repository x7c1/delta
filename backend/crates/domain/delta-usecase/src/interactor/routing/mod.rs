//! The interactor's public surface, routed through the per-session actors.
//!
//! Every state-mutating (or runtime-state-reading) use case becomes "post one
//! [`SessionInput`] to the owning actor, await its reply", so the public
//! method signatures are unchanged while per-session ordering is enforced by
//! the actor mailbox instead of lock discipline. Pure store reads (listing,
//! threads, messages, workdir browsing) do **not** come through here — they
//! stay direct on the core (see the `listing`/`workdir` modules), reachable
//! via the interactor's `Deref`.

mod commands;
mod enqueue_send;
mod hooks;
mod permissions;
mod queries;
mod questions;
mod send_cancellation;
mod ticks;

#[cfg(test)]
mod testing;

use delta_model::SessionId;
use tokio::sync::oneshot;

use crate::error::{Error, Result};
use crate::interactor::session_actor::input::{Reply, SessionInput};
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
    /// Post an input to the session's actor (spawning it on first contact)
    /// and await the result.
    async fn request<R>(
        &self,
        id: &SessionId,
        make: impl FnOnce(Reply<R>) -> SessionInput,
    ) -> Result<R> {
        let (tx, rx) = oneshot::channel();
        self.sessions.post(id, make(tx));
        match rx.await {
            Ok(result) => result,
            // Only reachable during tear-down (the input was dropped) or if
            // the actor panicked mid-handling.
            Err(_) => Err(Error::Internal(format!(
                "session {id} actor dropped before replying"
            ))),
        }
    }

    /// Read a piece of runtime state from the session's actor, substituting
    /// `default` when the session has no actor (closed/idle by definition).
    async fn query<R>(
        &self,
        id: &SessionId,
        make: impl FnOnce(oneshot::Sender<R>) -> SessionInput,
        default: R,
    ) -> R {
        let (tx, rx) = oneshot::channel();
        if !self.sessions.post_existing(id, make(tx)) {
            return default;
        }
        rx.await.unwrap_or(default)
    }

    /// Mint a fresh Claude `session_id` for a spawn: a time-ordered UUID v7
    /// (a 48-bit millisecond timestamp prefix followed by random bits), so
    /// session ids sort chronologically by creation time while remaining
    /// fully valid RFC 9562 UUIDs, and collision with an existing stored
    /// session is astronomically unlikely. Minted here — before the session's
    /// actor exists — because the id *is* the actor's routing key.
    fn mint_session_id() -> SessionId {
        SessionId::from(uuid::Uuid::now_v7().to_string())
    }
}
