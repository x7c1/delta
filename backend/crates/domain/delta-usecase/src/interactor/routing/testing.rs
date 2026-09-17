//! Test-only seams over the per-session runtime state.
//!
//! These replace the lock-era seams that reached into the shared
//! registries: each runs a closure inside the owning actor (in mailbox
//! order, like any real input), so tests can seed launch state with
//! controlled timestamps and read it back without widening the
//! production surface.

use std::time::Instant;

use delta_model::SessionId;
use tokio::sync::oneshot;

use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::session_actor::runtime::{
    AttachablePane, OpenHandle, PendingSpawn, ResumingSession, SessionRuntime,
};
use crate::interactor::Interactor;
use crate::pane_token::PaneToken;
use crate::ports::{pane_for, GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Run a closure against a session's runtime state (spawning its actor
    /// if absent), returning the closure's result.
    pub(crate) async fn with_runtime<R: Send + 'static>(
        &self,
        id: &SessionId,
        f: impl FnOnce(&mut SessionRuntime) -> R + Send + 'static,
    ) -> R {
        let (tx, rx) = oneshot::channel();
        self.sessions.post(
            id,
            SessionInput::WithRuntime(Box::new(move |state| {
                let _ = tx.send(f(state));
            })),
        );
        rx.await.expect("session actor dropped")
    }

    /// Like [`Self::with_runtime`], but never spawns an actor: returns
    /// `None` when the session has none (i.e. default runtime state).
    async fn with_runtime_existing<R: Send + 'static>(
        &self,
        id: &SessionId,
        f: impl FnOnce(&mut SessionRuntime) -> R + Send + 'static,
    ) -> Option<R> {
        let (tx, rx) = oneshot::channel();
        let posted = self.sessions.post_existing(
            id,
            SessionInput::WithRuntime(Box::new(move |state| {
                let _ = tx.send(f(state));
            })),
        );
        if !posted {
            return None;
        }
        rx.await.ok()
    }

    /// Wait until no session's launch preparation is still running *and*
    /// every actor has applied its report.
    ///
    /// A new-session send is answered *before* its launch is prepared: the
    /// worktree build, the trust seed and the tmux launch run on a
    /// background task. So a test that asserts on what the launch did —
    /// the created worktree, the spawned pane, the recorded pending spawn —
    /// has to let that task finish first. This is that wait, and it is the
    /// deterministic alternative to sleeping.
    ///
    /// It deliberately does not poll session state: the launch's records
    /// move on their own schedule (the pending spawn is recorded mid-launch,
    /// and the launch's first hook may consume it before the task reports
    /// back), so no snapshot of that state means "the task is done". It
    /// waits on [`InteractorCore::launches_in_flight`] instead, then flushes
    /// every live actor's mailbox with a round-trip — the counter drops once
    /// `LaunchFinished` has been *posted*, and the mailbox is FIFO, so
    /// anything posted after that is handled after it.
    ///
    /// Panics rather than returning on timeout: a launch that never reports
    /// back is a bug in the code under test, not a slow machine.
    pub(crate) async fn await_launch(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while self
            .launches_in_flight
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0
        {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for a launch preparation to finish"
            );
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        for id in self.sessions.ids() {
            self.with_runtime_existing(&id, |_| ()).await;
        }
    }

    /// The session ids whose launch preparation is still in flight —
    /// accepted, with no pane yet.
    pub(crate) async fn launching_session_ids(&self) -> Vec<SessionId> {
        let mut found = Vec::new();
        for id in self.sessions.ids() {
            let launching = self
                .with_runtime_existing(&id, |state| state.launching_spawn().is_some())
                .await
                .unwrap_or(false);
            if launching {
                found.push(id);
            }
        }
        found
    }

    /// The Delta-minted session ids of the currently-pending spawns, in
    /// spawn order.
    ///
    /// A fresh spawn's session id is a random UUID a test cannot predict,
    /// yet it is the hook-binding key. Tests spawn, read the id(s) back
    /// here, then fire a `UserPromptSubmit` carrying that exact id to
    /// bind. Spawn order is recovered from the pane token's monotonic
    /// mint ordinal (`delta-<n>`), since the pending entries now live one
    /// per actor.
    ///
    /// A spawn only becomes *pending* partway through its background launch,
    /// so this awaits that launch first — a test reading ids back here wants
    /// the spawn the hooks will address, not the accept-time state. A test
    /// that deliberately holds a launch open reads
    /// [`Self::launching_session_ids`] instead.
    pub(crate) async fn pending_session_ids(&self) -> Vec<SessionId> {
        self.await_launch().await;
        let mut found = Vec::new();
        for id in self.sessions.ids() {
            let ordinal = self
                .with_runtime_existing(&id, |state| {
                    state
                        .pending_spawn()
                        .map(|spawn| token_ordinal(spawn.token.as_str()))
                })
                .await
                .flatten();
            if let Some(ordinal) = ordinal {
                found.push((ordinal, id));
            }
        }
        found.sort_by_key(|(ordinal, _)| *ordinal);
        found.into_iter().map(|(_, id)| id).collect()
    }

    /// The pane a session is **bound** to, or `None` while it is not.
    ///
    /// The narrow question most tests mean when they ask about a session's
    /// pane: did this hook bind it, does closing it release it. The
    /// attachable-pane lookup ([`Self::pane_for_session`]) answers a wider
    /// one — it also resolves a spawn whose pane is up but unbound, which is
    /// what the PTY bridge attaches to — so a bind assertion phrased against
    /// it would pass before the bind it is checking for.
    pub(crate) async fn bound_pane(&self, id: &SessionId) -> Option<String> {
        self.with_runtime_existing(id, |state| state.handle().map(|handle| handle.pane.clone()))
            .await
            .flatten()
    }

    /// What [`Interactor::attach_pane`] would resolve for `id`, without
    /// recording an attach.
    ///
    /// A seam rather than production API: the bridge is the only caller
    /// that needs this lookup and it always wants the record, so a second
    /// public method resolving the same thing would be a surface nothing
    /// asks for. Tests want the read alone — the attach they are not making
    /// would itself hold the watchdog off the pane they are asserting about.
    pub(crate) async fn pane_for_session(&self, id: &SessionId) -> Option<AttachablePane> {
        self.query(id, |reply| SessionInput::QueryPane { reply }, None)
            .await
    }

    /// Record a pending spawn with an explicit `created_at`, for watchdog
    /// tests.
    ///
    /// The production `created_at` is `Instant::now()` at the instant the
    /// background launch is about to create the pane (not at acceptance —
    /// see `record_launched_pane`), which a test cannot wind backwards.
    /// Reaper tests
    /// instead push a spawn stamped at a chosen instant (e.g. `now - 31s`)
    /// and then call `reap_stale_spawns(now)` so the deadline check is
    /// fully deterministic.
    pub(crate) async fn push_pending_spawn_at(
        &self,
        token: &str,
        session_id: &SessionId,
        created_at: Instant,
    ) {
        let token = token.to_owned();
        self.with_runtime(session_id, move |state| {
            state.push_pending(PendingSpawn {
                token: PaneToken::from_raw(&token),
                pane: pane_for(&token),
                created_at,
                // The seam stands in for a launch that got as far as
                // creating its pane, which is the state every watchdog and
                // attach test is about.
                pane_created: true,
            });
        })
        .await;
    }

    /// Bind a live, ready pane for a session, as if it had been spawned
    /// and become ready.
    ///
    /// Most enqueue/defer tests register `sess-1` then send to it, and
    /// want it to behave like a normal *open and ready* session (sends
    /// dispatch immediately). Registering via `on_user_prompt_submit`
    /// alone marks it known-but-closed, so the next send would resume it
    /// and — under the readiness gate — hold the first keystroke. This
    /// seam binds a ready pane up front so those tests exercise the
    /// immediate-dispatch path, not the resume gate (which has its own
    /// focused tests).
    pub(crate) async fn bind_open_session(&self, token: &str, session_id: &SessionId) {
        let token = token.to_owned();
        self.with_runtime(session_id, move |state| {
            state.bind(OpenHandle {
                token: PaneToken::from_raw(&token),
                pane: pane_for(&token),
            });
        })
        .await;
    }

    /// The session ids currently resuming-but-not-ready, for resume-gate
    /// tests.
    pub(crate) async fn resuming_session_ids(&self) -> Vec<SessionId> {
        let mut found = Vec::new();
        for id in self.sessions.ids() {
            let resuming = self
                .with_runtime_existing(&id, |state| state.resuming().is_some())
                .await
                .unwrap_or(false);
            if resuming {
                found.push(id);
            }
        }
        found
    }

    /// Apply a turn input directly to a session's state machine, for tests
    /// that seed a specific turn state (e.g. a held prompt's outstanding
    /// dispatch). State-only: the seeding transitions used by tests orphan
    /// nothing, so no store disposition runs here.
    pub(crate) async fn apply_turn_input(
        &self,
        id: &SessionId,
        input: crate::turn::TurnInput,
    ) -> crate::error::Result<crate::turn::TurnState> {
        Ok(self
            .with_runtime(id, move |state| state.apply_turn(input).next)
            .await)
    }

    /// Mark a resuming session ready at an explicit instant, for
    /// resume-dispatch tests. Returns whether the id was resuming (the
    /// production hook's return).
    pub(crate) async fn mark_resume_ready_at(&self, id: &SessionId, ready_at: Instant) -> bool {
        self.with_runtime(id, move |state| state.mark_resume_ready_at(ready_at))
            .await
    }

    /// Record a resuming (not-yet-ready) session with an explicit
    /// `created_at`, for resume-watchdog tests. A resuming session is
    /// also bound (its pane exists), so this mirrors production: bind the
    /// handle and record the resuming entry together.
    pub(crate) async fn push_resuming_at(
        &self,
        token: &str,
        session_id: &SessionId,
        held_prompt: Option<String>,
        created_at: Instant,
    ) {
        let token = token.to_owned();
        self.with_runtime(session_id, move |state| {
            state.bind(OpenHandle {
                token: PaneToken::from_raw(&token),
                pane: pane_for(&token),
            });
            state.start_resuming(ResumingSession {
                token: PaneToken::from_raw(&token),
                pane: pane_for(&token),
                held_prompt,
                created_at,
                ready_at: None,
            });
        })
        .await;
    }
}

/// The numeric suffix of a minted `delta-<n>` token, for spawn ordering;
/// raw test tokens without one sort first.
fn token_ordinal(token: &str) -> u64 {
    token
        .rsplit('-')
        .next()
        .and_then(|suffix| suffix.parse().ok())
        .unwrap_or(0)
}
