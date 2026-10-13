//! The connection-level drain of the frames that belong to no thread.

use std::sync::Arc;

use tokio::sync::mpsc;

use delta_usecase::CommsLogSink;

use super::{record_inbound, WeakSessionMap};
use crate::translate::AccountRateLimits;
use crate::ThreadEvent;

/// The connection-level drain: fold the frames that belong to no thread into
/// neutral events and fan them out to every live session.
///
/// `account/rateLimits/updated` is the reason this exists. It carries no
/// `threadId` — the account it describes is shared by every thread on the
/// connection — so the transport routes it here instead of to a session. The
/// facts it produces are account-scoped, and the browser keys them by provider,
/// so surfacing the same fact on each live session is idempotent rather than
/// conflicting: whichever session's pump runs first tells the browser the truth,
/// and the rest repeat it.
///
/// Fan-out to zero sessions drops the fact rather than deferring it. The
/// session map is empty between `initialize` and the connection's first
/// `launch` or `resume`, so an account update that lands in that gap reaches no
/// one. The frame still updates the merged state (see [`AccountRateLimits`]),
/// but that state is never replayed to a session registered later, so such a
/// session shows no rate-limit rows until the server pushes its next rolling
/// update — and whether one comes is the server's cadence to decide, not
/// something this loop can guarantee.
///
/// A frame that yields no neutral event is logged and dropped, never re-queued:
/// the whole point of draining is that nothing accumulates here unseen. Two
/// kinds of frame yield nothing: a method this build does not model, and a
/// rate-limit frame that states nothing (no `rateLimits` object at all). A
/// server → client *request* with no thread takes the same path — logged
/// rather than answered, because answering a request whose session is unknown
/// is a guess.
///
/// Every frame — translated or not — is also mirrored into each live session's
/// comms log. The connection cannot attribute an account frame to one session,
/// but the operator reading the inspector still needs to see the frame that
/// moved the rate-limit rows; showing it in every open Codex session's log is
/// the honest rendering of "this arrived on the connection you are watching".
pub(super) async fn account_loop(
    unrouted: Option<mpsc::Receiver<ThreadEvent>>,
    comms: Arc<dyn CommsLogSink>,
    sessions: WeakSessionMap,
) {
    let Some(mut unrouted) = unrouted else {
        // Someone else already took the channel. Nothing to drain, and nothing
        // this loop could do about it.
        eprintln!("codex-agent: the unrouted channel was already taken; account frames are lost");
        return;
    };
    let mut rate_limits = AccountRateLimits::default();
    while let Some(frame) = unrouted.recv().await {
        let events = match &frame {
            ThreadEvent::Notification(notification) => rate_limits.translate(notification),
            // A connection-level *request* is a guess to answer (see the docs
            // above). `ConnectionLost` never reaches this channel at all — the
            // transport announces it per thread, and this loop ends anyway when
            // the reader drops the unrouted sender — so it is grouped here
            // rather than given a fan-out of its own, which would tell every
            // session's pump the connection died twice.
            ThreadEvent::ServerRequest(_) | ThreadEvent::ConnectionLost => Vec::new(),
        };
        if events.is_empty() {
            eprintln!(
                "codex-agent: dropping a connection-level frame that produced no event: `{}`",
                frame_method(&frame)
            );
        }
        let Some(sessions) = sessions.upgrade() else {
            return;
        };
        for session in sessions.lock().expect("sessions mutex poisoned").values() {
            record_inbound(&comms, &session.session_id, &frame);
            for event in &events {
                // A closed receiver means that session's pump is gone; the
                // account fact is not its alone, so the loop carries on.
                let _ = session.tx.send(event.clone());
            }
        }
    }
}

/// The method name of a connection-level frame, for the drop log. A lost
/// connection names no method, so it reads as itself.
fn frame_method(frame: &ThreadEvent) -> &str {
    match frame {
        ThreadEvent::Notification(notification) => &notification.method,
        ThreadEvent::ServerRequest(request) => &request.method,
        ThreadEvent::ConnectionLost => "<connection lost>",
    }
}
