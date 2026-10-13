//! The send FIFO: enqueue, dispatch, hold, and settle transitions.

use async_trait::async_trait;

use delta_model::{MessageUuid, Send, SessionId, ThreadId};

use super::SessionStore;
use crate::error::Result;

/// The send FIFO: every message the user composed, from enqueue through
/// dispatch to its match, hold, or cancellation.
///
/// One of the concerns [`SessionStore`] gathers.
// `delta_model::Send` (imported above) shadows the `std::marker::Send` prelude
// trait in this module, so the auto-trait bound is spelled out explicitly.
#[async_trait]
pub trait SendStore: std::marker::Send + Sync {
    /// Record a send in the `dispatched` state (its keystrokes are about to be
    /// typed into the pane) and return the created row.
    async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send>;

    /// Record a send held in the `queued` state: its row is recorded (so the
    /// branch thread and the queued text persist) but its keystrokes are not
    /// dispatched yet. Delta promotes it with [`Self::promote_queued_send`]
    /// and dispatches it once the session goes idle.
    async fn enqueue_queued_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send>;

    /// A single send by id, if it exists.
    ///
    /// Used to resolve the owning session of a cancel request (which carries
    /// only the send id in its URL) so the cancel can route through that
    /// session's actor, ordered against the dispatch path.
    async fn send(&self, id: i64) -> Result<Option<Send>>;

    /// The oldest still-`queued` send for a session (FIFO), if any. This is
    /// the next held-back send to dispatch when the session becomes idle.
    ///
    /// Rows carrying the hold marker (`held_at`) are skipped: a held send
    /// must never dispatch automatically — not at resume settle, not at turn
    /// end, not on the enqueue idle-flush — until the user explicitly releases
    /// it via [`Self::release_held_send`]. Both producers of the marker
    /// ([`Self::restore_all_dispatched`] and [`Self::hold_send_for_release`])
    /// rely on this skip.
    async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>>;

    /// A session's open (non-terminal) sends — status `queued` or
    /// `dispatched` — oldest first (ascending `id`).
    ///
    /// This is the server-side truth behind the browser's send strip:
    /// every send accepted for the session that has neither matched a
    /// transcript line nor been cancelled yet. Held rows (see
    /// [`Self::restore_all_dispatched`] and [`Self::hold_send_for_release`])
    /// are included — they are `queued` — and carry their `held_at` marker
    /// so the UI can render them with the explicit Send/Cancel affordances
    /// instead of a waiting label.
    async fn open_sends(&self, session_id: &SessionId) -> Result<Vec<Send>>;

    /// Promote a `queued` send to `dispatched`, marking it typed so the
    /// normal `UserPromptSubmit` correlation can match it.
    async fn promote_queued_send(&self, id: i64) -> Result<()>;

    /// Return a `dispatched` send to `queued`. A no-op for any other status.
    ///
    /// Used when the turn state machine orphans an outstanding send whose echo
    /// never arrived (see `OrphanedSend::Requeue`): the row keeps its
    /// thread/branch/quote semantics and re-dispatches on the next trigger
    /// that finds the session open and idle — a turn end, an interrupt
    /// ingest, a resume settle, or the enqueue idle-flush (see the use case's
    /// `dispatch_queued_send`) — so a composed message is never silently
    /// lost.
    async fn requeue_send(&self, id: i64) -> Result<()>;

    /// Restore **every** `dispatched` send — across all sessions — to
    /// `queued` with the `held_at` marker set, returning how many rows
    /// transitioned. Rows in any other status are untouched.
    ///
    /// The boot-time half of the single-outstanding invariant: turn state is
    /// runtime-only and rebuilt `Idle` when the server starts, but `send` rows
    /// are persistent — so a row that was `dispatched` when the previous
    /// process died has no turn machine awaiting its echo and would shadow
    /// [`Self::head_dispatched_send`] correlation forever. The composition
    /// root calls this exactly once at startup, before any session actor
    /// exists (which is what makes the blanket sweep exact: at that moment
    /// every `dispatched` row is an orphan by definition). Restored rather
    /// than cancelled so a composed message is never silently lost — but,
    /// unlike [`Self::requeue_send`]'s plain requeue, a restored row is never
    /// re-dispatched automatically: the message may be days old and the
    /// conversation has moved on, so auto-resending it on the next reopen
    /// would silently re-submit stale text (possibly *after* a newer message
    /// the user just sent). The `held_at` marker keeps the row out of
    /// [`Self::next_queued_send`] until the user explicitly releases it
    /// ([`Self::release_held_send`]) or cancels it.
    ///
    /// This is the *boot* producer of that marker; the other is
    /// [`Self::hold_send_for_release`].
    async fn restore_all_dispatched(&self) -> Result<usize>;

    /// Return **one** still-`dispatched` send to `queued` with the hold marker
    /// set, returning whether a row actually transitioned.
    ///
    /// The single-row sibling of [`Self::restore_all_dispatched`], and the
    /// marker's other producer: the echo-deadline park (see
    /// `SessionContext::park_unechoable_send`) uses it for a send that has
    /// spent its requeue budget — nothing was ever heard about it, so it must
    /// not be re-typed automatically, but the composed message is still worth
    /// keeping. The row stays in [`Self::open_sends`], out of
    /// [`Self::next_queued_send`], and waits for the user's explicit release
    /// ([`Self::release_held_send`]) or cancel, exactly like a
    /// boot-restored row.
    ///
    /// The `WHERE status = 'dispatched'` guard makes it a no-op (returning
    /// `false`) for a row that has since matched, settled, or been cancelled,
    /// or that does not exist — a held row is only ever made out of a genuinely
    /// outstanding one, never out of a terminal one.
    async fn hold_send_for_release(&self, id: i64) -> Result<bool>;

    /// Clear the hold marker of a send, returning it to the normal queued flow
    /// — but **only while it is still `queued` and held** — returning whether a
    /// row actually transitioned.
    ///
    /// The guarded release half of both marker producers
    /// ([`Self::restore_all_dispatched`] and [`Self::hold_send_for_release`]):
    /// the `WHERE status = 'queued' AND held_at IS NOT NULL` clause makes
    /// the transition a no-op (returning `false`) for an unknown id, a row that
    /// was never held, an already-released row, or one that has since been
    /// cancelled — a clean conflict rather than a clobber. After a successful
    /// release the row is an ordinary `queued` send again and dispatches
    /// through the usual idle triggers.
    async fn release_held_send(&self, id: i64) -> Result<bool>;

    /// Settle a **held** send as delivered, attributed to the transcript line
    /// `matched_uuid` — moving it to `matched` (and clearing its hold marker)
    /// only while it is still `queued` and held — returning whether a row
    /// actually transitioned.
    ///
    /// The held-row sibling of [`Self::mark_send_matched`]. A row the boot
    /// restore held ([`Self::restore_all_dispatched`]) was `dispatched` when
    /// the previous process stopped: its keystrokes may well have reached a
    /// pane that outlived that process and submitted there. When the pane is
    /// re-adopted and its transcript catch-up shows the prompt, the message was
    /// delivered, and leaving the row held would invite the user to type it a
    /// second time. The guard makes this a no-op for a row that was released,
    /// cancelled, or already settled in the meantime.
    async fn settle_held_send(&self, id: i64, matched_uuid: &MessageUuid) -> Result<bool>;

    /// The outstanding dispatched send for a session, if any.
    ///
    /// Under the single-outstanding dispatch rule at most one `dispatched` row
    /// exists per session, so this *is* the send both `UserPromptSubmit`
    /// correlation and transcript attribution work from, and both use it the
    /// same way — by position. The hook **consumes** this send: its keystrokes
    /// are already in the pane, so the prompt that submits is its own whatever
    /// text the hook reports (Claude Code rewrites prompts), and no comparison
    /// happens there. The one exception is the **resume window**: a first send
    /// against a resuming session is `dispatched` while its keystrokes are
    /// still *held* (typing into a pane that is not accepting input would lose
    /// them), so a prompt arriving then cannot be its echo and must not consume
    /// it — the resume path checks the held prompt before consuming. Outside
    /// that window "dispatched" does mean "already typed".
    ///
    /// Attribution then **claims** it for the first human user
    /// line ingested after the dispatch, again whatever that line says, binding
    /// the row through [`Self::mark_send_matched`]; trimmed-text equality only
    /// reports whether the echo came back verbatim. The row is left for
    /// [`Self::settle_send_delivered`] to finish at turn end only when no such
    /// line was ingested at all. Defined as the oldest `dispatched` row so
    /// that, should the invariant ever be violated, both deterministically pick
    /// the earliest.
    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>>;

    /// All `dispatched` sends for a session, oldest first (ascending `id`).
    ///
    /// The single-outstanding rule normally caps this list at one element, so
    /// callers that want the head should still prefer
    /// [`Self::head_dispatched_send`]. This is the recovery accessor used when
    /// a re-dispatch routine needs to re-type the *entire* FIFO of currently
    /// `Dispatched` sends — e.g. after Claude Code's auto- or manual `/compact`
    /// swallowed each typed prompt without echoing, leaving any dispatched
    /// row stuck behind a missing echo.
    async fn dispatched_sends(&self, session_id: &SessionId) -> Result<Vec<Send>>;

    /// Mark a dispatched send matched to a transcript message uuid.
    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()>;

    /// Settle a send as **delivered** without a transcript uuid — moving it to
    /// `matched` only while it is still `dispatched` — returning whether a row
    /// actually transitioned.
    ///
    /// The unattributed sibling of [`Self::mark_send_matched`]. Both
    /// consumption and attribution are decided by *position*: a prompt
    /// submitted while that send was the outstanding one consumes it, and the
    /// first human transcript line ingested afterwards claims it — a rewritten
    /// echo included. What is left for this call is the case where no such line
    /// ever arrived: the turn ended (or a `/compact` swallowed the echo) before
    /// any user line was ingested. The message was still delivered. Cancelling
    /// such a row would record a delivered message as undelivered (and
    /// re-surface it to the user as a failure); leaving it `dispatched` would
    /// shadow [`Self::head_dispatched_send`] for the next send. So it settles
    /// as `matched` with `matched_uuid` left `NULL`: delivered, with no
    /// transcript line claimed.
    ///
    /// The `WHERE status = 'dispatched'` guard makes it a no-op (returning
    /// `false`) for a row that already matched its line, was cancelled or
    /// parked, or does not exist — this only ever finishes a still-outstanding
    /// row, never revives a terminal one.
    async fn settle_send_delivered(&self, id: i64) -> Result<bool>;

    /// Cancel a send by marking the row `cancelled`.
    ///
    /// The row is kept (rather than deleted) for audit, and because
    /// [`Self::head_dispatched_send`] only considers `dispatched` rows, a
    /// cancelled row no longer blocks the FIFO head. Used to roll back a
    /// `dispatched` row whose keystrokes were never delivered (e.g. a failed
    /// tmux dispatch).
    async fn cancel_send(&self, id: i64) -> Result<()>;

    /// Cancel a send **only while it is still `queued`**, returning whether a
    /// row actually transitioned.
    ///
    /// The guarded sibling of [`Self::cancel_send`]: a user-initiated cancel may
    /// only abandon a send that has not yet been typed into the pane, so the
    /// `WHERE status = 'queued'` clause makes the transition a no-op (returning
    /// `false`) the moment the send has been dispatched, matched, or already
    /// cancelled — losing a race with the idle dispatch path rather than
    /// clobbering an in-flight send. A cancelled (terminal) row drops out of
    /// [`Self::open_sends`] and is skipped by [`Self::next_queued_send`], so it
    /// is never dispatched on idle.
    async fn cancel_queued_send(&self, id: i64) -> Result<bool>;
}

#[async_trait]
impl SendStore for Box<dyn SessionStore> {
    async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        (**self)
            .enqueue_send(
                session_id,
                thread_id,
                semantic_parent_uuid,
                text,
                locator_quote,
            )
            .await
    }

    async fn enqueue_queued_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        (**self)
            .enqueue_queued_send(
                session_id,
                thread_id,
                semantic_parent_uuid,
                text,
                locator_quote,
            )
            .await
    }

    async fn send(&self, id: i64) -> Result<Option<Send>> {
        (**self).send(id).await
    }

    async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        (**self).next_queued_send(session_id).await
    }

    async fn open_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        (**self).open_sends(session_id).await
    }

    async fn promote_queued_send(&self, id: i64) -> Result<()> {
        (**self).promote_queued_send(id).await
    }

    async fn requeue_send(&self, id: i64) -> Result<()> {
        (**self).requeue_send(id).await
    }

    async fn restore_all_dispatched(&self) -> Result<usize> {
        (**self).restore_all_dispatched().await
    }

    async fn hold_send_for_release(&self, id: i64) -> Result<bool> {
        (**self).hold_send_for_release(id).await
    }

    async fn release_held_send(&self, id: i64) -> Result<bool> {
        (**self).release_held_send(id).await
    }

    async fn settle_held_send(&self, id: i64, matched_uuid: &MessageUuid) -> Result<bool> {
        (**self).settle_held_send(id, matched_uuid).await
    }

    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        (**self).head_dispatched_send(session_id).await
    }

    async fn dispatched_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        (**self).dispatched_sends(session_id).await
    }

    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()> {
        (**self).mark_send_matched(id, matched_uuid).await
    }

    async fn settle_send_delivered(&self, id: i64) -> Result<bool> {
        (**self).settle_send_delivered(id).await
    }

    async fn cancel_send(&self, id: i64) -> Result<()> {
        (**self).cancel_send(id).await
    }

    async fn cancel_queued_send(&self, id: i64) -> Result<bool> {
        (**self).cancel_queued_send(id).await
    }
}
