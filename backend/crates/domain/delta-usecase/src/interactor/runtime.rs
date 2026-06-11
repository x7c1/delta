//! Simple runtime getters over the interactor's injected state.
//!
//! EXCEPTION to the one-method-per-file rule: these are trivial accessors, so
//! they are grouped together rather than each given its own file.

use delta_model::SessionId;

use crate::error::Result;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Borrow the store (useful for read-only queries from the transport layer).
    pub fn store(&self) -> &S {
        &self.store
    }

    /// The pane driving a specific open session, if it is open.
    ///
    /// The PTY bridge routes by session id: with no live pane for `id` this
    /// returns `None` so the bridge can refuse the attach rather than bind to a
    /// non-existent pane.
    pub async fn pane_for_session(&self, id: &SessionId) -> Option<String> {
        self.open_sessions
            .lock()
            .await
            .handle(id)
            .map(|h| h.pane.clone())
    }

    /// Wipe the residual input of a session's pane, if it is open.
    ///
    /// Resolves the session's live pane from the registry exactly like
    /// [`Self::pane_for_session`] does. When the session is open the pane's
    /// current input is cleared via the driver; when it is not open there is no
    /// live pane to clear, so this is a no-op returning `Ok(())`.
    ///
    /// Intended for use right before a fresh PTY attach: a prior client's detach
    /// leaves a focus-out (`ESC[O`) that Claude renders as a stray blank line, so
    /// clearing on the next attach keeps the input box clean across reconnects.
    pub async fn clear_session_input(&self, id: &SessionId) -> Result<()> {
        let pane = self
            .open_sessions
            .lock()
            .await
            .handle(id)
            .map(|h| h.pane.clone());
        if let Some(pane) = pane {
            self.tmux.clear_input(&pane).await?;
        }
        Ok(())
    }

    /// Whether a session is currently open (driven by a live pane).
    ///
    /// Open/closed is process-runtime state held by the registry, so this is the
    /// authority the session-list endpoint annotates each stored session with.
    pub async fn is_session_open(&self, id: &SessionId) -> bool {
        self.open_sessions.lock().await.is_open(id)
    }

    #[cfg(test)]
    pub(crate) fn transcript(&self) -> &X {
        &self.transcript
    }

    #[cfg(test)]
    pub(crate) fn tmux(&self) -> &T {
        &self.tmux
    }

    #[cfg(test)]
    pub(crate) fn workspace(&self) -> &W {
        &self.workspace
    }

    /// The Delta-minted session ids of the currently-pending spawns, in order.
    ///
    /// Test-only seam: a fresh spawn's session id is a random UUID a test cannot
    /// predict, yet it is now the hook-binding key. Tests spawn, read the id(s)
    /// back here, then fire a `UserPromptSubmit` carrying that exact id to bind.
    #[cfg(test)]
    pub(crate) async fn pending_session_ids(&self) -> Vec<SessionId> {
        self.open_sessions.lock().await.pending_session_ids()
    }

    /// Record a pending spawn with an explicit `created_at`, for watchdog tests.
    ///
    /// Test-only seam: the production `created_at` is `Instant::now()` at spawn
    /// time, which a test cannot wind backwards. Reaper tests instead push a
    /// spawn stamped at a chosen instant (e.g. `now - 31s`) and then call
    /// `reap_stale_spawns(now)` so the deadline check is fully deterministic.
    #[cfg(test)]
    pub(crate) async fn push_pending_spawn_at(
        &self,
        token: &str,
        session_id: &SessionId,
        created_at: std::time::Instant,
    ) {
        use crate::open_sessions::PendingSpawn;
        use crate::pane_token::PaneToken;
        use crate::ports::pane_for;
        self.open_sessions.lock().await.push_pending(PendingSpawn {
            token: PaneToken::from_raw(token),
            pane: pane_for(token),
            session_id: session_id.clone(),
            workdir: "/work".to_owned(),
            first_prompt: None,
            created_at,
        });
    }
}
