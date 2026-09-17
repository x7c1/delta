//! Runtime-state queries: the process-runtime facts only a session's actor
//! knows — the pane a PTY bridge may attach to, whether the session is open or
//! live, and its turn phase with the pending permission queue. Each read
//! substitutes a documented default when the session has no actor.
//! `detach_pane` is grouped here as the write that closes out an attach
//! `attach_pane` handed out.

use std::time::Instant;

use delta_model::SessionId;
use tokio::sync::oneshot;

use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::session_actor::runtime::{
    AttachablePane, SessionListingState, SessionLiveState,
};
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::TurnState;

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// The pane a PTY bridge may attach to for `id`, and whether the session is
    /// bound to it, with the record that a bridge is now attached to what it
    /// returns.
    ///
    /// Routed by session id to the actor's `SessionRuntime::attachable_pane`,
    /// which defines what resolves and what does not (and why an unbound
    /// spawn's pane does).
    ///
    /// What the record buys: the launch watchdog leaves an unbound spawn's pane
    /// alone while somebody is attached to it
    /// (`SessionRuntime::take_stale_pending`).
    ///
    /// Every `Some` **must** be followed by a [`Self::detach_pane`] when the
    /// bridge ends, or the pane stays unreapable for the life of the actor.
    pub async fn attach_pane(&self, id: &SessionId) -> Option<AttachablePane> {
        self.query(id, |reply| SessionInput::AttachPane { reply }, None)
            .await
    }

    /// Record that a PTY bridge handed a pane by [`Self::attach_pane`] is gone
    /// as of `now`.
    ///
    /// A no-op for a session with no actor — it has no bookkeeping left to
    /// correct — so a bridge outliving its session's actor detaches harmlessly.
    ///
    /// `now` is the caller's, as for the tick drains: the last bridge leaving
    /// restarts an unbound spawn's bind deadline
    /// (`SessionRuntime::note_pty_detached`), so the bridge's exit and the
    /// watchdog's sweeps are read off one clock the tests control.
    pub async fn detach_pane(&self, id: &SessionId, now: Instant) {
        self.sessions
            .post_existing(id, SessionInput::DetachPane { now });
    }

    /// The runtime facts the session list annotates one stored row with —
    /// whether the session is open, and whether it holds an attachable pane
    /// nothing has bound yet — read in a single actor message.
    ///
    /// Both are process-runtime state owned by the session's actor, so this is
    /// the authority the session-list endpoint annotates each stored session
    /// with. One query rather than two so the pair is a consistent snapshot:
    /// the bind that flips `pane_starting` off is the same one that flips
    /// `open` on, and between two round-trips a row could carry neither.
    ///
    /// A session with no actor is closed with no starting pane by definition.
    pub(crate) async fn listing_state_for(&self, id: &SessionId) -> SessionListingState {
        self.query(
            id,
            |reply| SessionInput::QueryListingState { reply },
            SessionListingState {
                open: false,
                pane_starting: false,
            },
        )
        .await
    }

    /// Whether a session is currently open (driven by a live pane).
    ///
    /// The `open` half of [`Self::listing_state_for`], for the callers that
    /// only ask that one question.
    pub async fn is_session_open(&self, id: &SessionId) -> bool {
        self.listing_state_for(id).await.open
    }

    /// The ids of every session whose actor reports a live pane, in registry
    /// order.
    ///
    /// "Live" is wider than open: it also covers a spawn still in flight — one
    /// whose pane is up awaiting its first bind, and one that has only been
    /// accepted with its launch preparation still running
    /// (`SessionRuntime::has_live_pane`). A session with no actor is not live by
    /// definition, so the result is bounded by the number of live panes rather
    /// than by the size of the store.
    ///
    /// Two callers depend on that definition: the cold start (which reuses a
    /// live session instead of spawning a rival one) and the session list
    /// (whose open-first ordering leads with exactly this set).
    pub(crate) async fn live_session_ids(&self) -> Vec<SessionId> {
        let mut live = Vec::new();
        for id in self.sessions.ids() {
            if self
                .query(&id, |reply| SessionInput::QueryIsLive { reply }, false)
                .await
            {
                live.push(id);
            }
        }
        live
    }

    /// The queryable live state of a session: its turn phase plus the
    /// pending permission queue, snapshotted in one actor message.
    ///
    /// Public because the REST surface reports it (the sends envelope
    /// carries `turn` and `permission`, so the browser can rebuild its
    /// in-progress indicator and permission notice after a reconnect). A
    /// session with no actor is idle with nothing pending by definition.
    pub async fn live_state_for(&self, id: &SessionId) -> SessionLiveState {
        // Resolved inline (rather than through `query`) so each outcome can be
        // logged: a captured debug log must distinguish a state the live actor
        // actually returned from the default `Idle` substituted when no actor
        // is reachable. Without this, an `Idle` in a report is ambiguous —
        // genuinely idle, or a silent fallback?
        let default = SessionLiveState {
            turn: TurnState::Idle,
            in_progress_thread: None,
            pending_permissions: Vec::new(),
            pending_question: None,
            running_subagents: Vec::new(),
        };
        let (tx, rx) = oneshot::channel();
        if !self
            .sessions
            .post_existing(id, SessionInput::QueryLiveState { reply: tx })
        {
            tracing::debug!(
                session_id = %id,
                branch = "no_actor",
                "live_state_for: no session actor; returning default Idle (a session \
                 with no actor is idle with nothing pending by definition)"
            );
            return default;
        }
        match rx.await {
            Ok(state) => {
                tracing::debug!(
                    session_id = %id,
                    branch = "actor_reply",
                    turn = ?state.turn,
                    pending_permissions = state.pending_permissions.len(),
                    has_pending_question = state.pending_question.is_some(),
                    "live_state_for: state from live actor"
                );
                state
            }
            Err(_) => {
                tracing::debug!(
                    session_id = %id,
                    branch = "dropped_reply",
                    "live_state_for: actor existed but dropped its reply (retiring \
                     mid-query); returning default Idle"
                );
                default
            }
        }
    }
}
