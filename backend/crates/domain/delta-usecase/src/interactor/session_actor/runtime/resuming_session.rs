//! A resumed session whose first prompt is still held.

use std::time::Instant;

use crate::pane_token::PaneToken;

#[cfg(doc)]
use super::spawn::RESUME_DISPATCH_SETTLE;
#[cfg(doc)]
use super::{PendingSpawn, RESUME_READY_DEADLINE};

/// A resumed-but-not-yet-ready session: its pane is bound, but its first prompt
/// is held until the resume's `SessionStart` (`source=resume`) hook arrives and,
/// crucially, until a beat *after* that hook returns.
///
/// Unlike a fresh spawn, a resume's `session_id` is known up front, so the pane
/// binds immediately. But `claude --resume` needs a couple of seconds to replay
/// the transcript and make its TUI input ready, far longer than any fixed
/// settle could safely cover. So Delta does not type the first prompt at
/// resume time; it records the resume here.
///
/// Readiness arrives in two stages, tracked by [`Self::ready_at`]:
///
/// 1. **Mark ready** — `SessionStart(source=resume)` fires. That hook blocks
///    `claude` until its HTTP handler returns, so the handler must *not* type
///    the held prompt (the keystroke would land while `claude` is still inside
///    the hook and not accepting input, and be lost). It only stamps
///    [`Self::ready_at`] and returns immediately, unblocking `claude`.
/// 2. **Dispatch** — on a later background tick, once `now - ready_at` has
///    reached [`RESUME_DISPATCH_SETTLE`], the held keystroke is dispatched on
///    the normal `send_line` path. By then the hook has returned and `claude`
///    is input-ready, so the keystroke submits.
///
/// The `send` row for that first prompt is written normally (its thread,
/// branch, and locator-quote semantics are persisted up front) — only the
/// physical keystroke is held.
#[derive(Debug, Clone)]
pub struct ResumingSession {
    /// The Delta-minted tmux session name backing the resumed pane, killed by
    /// the watchdog if the resume never becomes ready.
    pub token: PaneToken,
    /// The pane the held first prompt is dispatched into once ready.
    pub pane: String,
    /// The held first prompt's keystroke text, if a send is waiting on this
    /// resume's readiness. A resume opened with no immediate send (e.g. an
    /// explicit `open_session` with no following dispatch) carries `None`.
    pub held_prompt: Option<String>,
    /// When the resume was recorded, for the readiness watchdog deadline. Like
    /// [`PendingSpawn::created_at`] this is a monotonic `Instant`, so the
    /// deadline check is immune to system-clock changes.
    pub created_at: Instant,
    /// When `SessionStart(source=resume)` marked this resume ready, or `None`
    /// while still not-ready.
    ///
    /// `None` means the readiness hook has not arrived: the held prompt is
    /// parked and the watchdog may reap the resume if [`RESUME_READY_DEADLINE`]
    /// passes. `Some(t)` means the hook fired at `t` and the resume is pending
    /// dispatch — the watchdog must leave it alone, and the dispatch tick types
    /// the held prompt once `now - t` reaches [`RESUME_DISPATCH_SETTLE`].
    pub ready_at: Option<Instant>,
}
