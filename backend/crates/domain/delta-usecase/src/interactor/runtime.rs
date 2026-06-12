//! Simple runtime getters over the interactor's injected state.
//!
//! EXCEPTION to the one-method-per-file rule: these are trivial accessors, so
//! they are grouped together rather than each given its own file.

use delta_model::SessionId;

use crate::error::Result;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::interactor::InteractorCore;

impl<T, X, S, W> InteractorCore<T, X, S, W>
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
            created_at,
        });
    }

    /// Bind a live, ready pane for a session, as if it had been spawned and
    /// become ready.
    ///
    /// Test-only seam: most enqueue/defer tests register `sess-1` then send to
    /// it, and want it to behave like a normal *open and ready* session (sends
    /// dispatch immediately). Registering via `on_user_prompt_submit` alone marks
    /// it known-but-closed, so the next send would resume it and — under the
    /// readiness gate — hold the first keystroke. This seam binds a ready pane up
    /// front so those tests exercise the immediate-dispatch path, not the resume
    /// gate (which has its own focused tests).
    #[cfg(test)]
    pub(crate) async fn bind_open_session(&self, token: &str, session_id: &SessionId) {
        use crate::open_sessions::OpenHandle;
        use crate::pane_token::PaneToken;
        use crate::ports::pane_for;
        self.open_sessions.lock().await.bind(
            session_id.clone(),
            OpenHandle {
                token: PaneToken::from_raw(token),
                pane: pane_for(token),
                workdir: "/work".to_owned(),
            },
        );
    }

    /// The session ids currently resuming-but-not-ready, for resume-gate tests.
    #[cfg(test)]
    pub(crate) async fn resuming_session_ids(&self) -> Vec<SessionId> {
        self.open_sessions.lock().await.resuming_session_ids()
    }

    /// Mark a resuming session ready at an explicit instant, for resume-dispatch
    /// tests.
    ///
    /// Test-only seam mirroring [`Self::push_resuming_at`]: the production
    /// `ready_at` is `Instant::now()` inside the `SessionStart(resume)` handler,
    /// which a test cannot wind forwards/backwards. Dispatch-settle tests stamp a
    /// chosen `ready_at` and then call `dispatch_ready_resumes(now)` with a
    /// controlled `now`, so the `RESUME_DISPATCH_SETTLE` gate is deterministic.
    /// Returns whether the id was resuming (the production hook's return).
    #[cfg(test)]
    pub(crate) async fn mark_resume_ready_at(
        &self,
        id: &SessionId,
        ready_at: std::time::Instant,
    ) -> bool {
        self.open_sessions
            .lock()
            .await
            .mark_resume_ready_at(id, ready_at)
    }

    /// Record a resuming (not-yet-ready) session with an explicit `created_at`,
    /// for resume-watchdog tests.
    ///
    /// Test-only seam mirroring [`Self::push_pending_spawn_at`]: the production
    /// `created_at` is `Instant::now()` at resume time, which a test cannot wind
    /// backwards. Resume-reaper tests push a resuming session stamped at a chosen
    /// instant and then call `reap_stale_spawns(now)` so the deadline check is
    /// deterministic.
    #[cfg(test)]
    pub(crate) async fn push_resuming_at(
        &self,
        token: &str,
        session_id: &SessionId,
        held_prompt: Option<String>,
        created_at: std::time::Instant,
    ) {
        use crate::open_sessions::{OpenHandle, ResumingSession};
        use crate::pane_token::PaneToken;
        use crate::ports::pane_for;
        let mut registry = self.open_sessions.lock().await;
        // A resuming session is also bound (its pane exists), so mirror
        // production: bind the handle and record the resuming entry together.
        registry.bind(
            session_id.clone(),
            OpenHandle {
                token: PaneToken::from_raw(token),
                pane: pane_for(token),
                workdir: "/work".to_owned(),
            },
        );
        registry.start_resuming(
            session_id.clone(),
            ResumingSession {
                token: PaneToken::from_raw(token),
                pane: pane_for(token),
                held_prompt,
                created_at,
                ready_at: None,
            },
        );
    }
}
