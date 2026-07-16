//! The interactor's asynchronous event-emission seam.
//!
//! Every [`SessionEvent`] Delta produces today is *returned synchronously* to
//! whoever drove the work — a hook handler returns its events to the HTTP
//! handler, `poll_transcript` returns its events to the server tail loop — and
//! that caller broadcasts them. That path stays exactly as it is.
//!
//! Some producers, though, emit events *over time, after the call that started
//! the work has already returned*. A push-based agent adapter (Codex's
//! app-server) surfaces turn/content frames on its own schedule, long after
//! `enqueue_send` returned to the browser, so there is no synchronous return
//! value left to fold them into. This seam is the channel those producers push
//! on: the interactor side holds an [`AsyncEventSink`] and emits onto it; the
//! server side owns the matching [`AsyncEventReceiver`], drains it in a
//! background task, and forwards each event to its broadcast — the same
//! `broadcast` the synchronous path already feeds.
//!
//! It is deliberately additive: the sink is optional on the interactor
//! ([`None`] by default), so every existing synchronous path — and every test
//! that never wires a sink — is untouched.

use tokio::sync::mpsc;

use super::SessionEvent;

/// The sending half of the async event seam, held by the interactor.
///
/// Cheap to clone (an `mpsc` sender is reference-counted): the interactor keeps
/// one and any number of its async producers may hold clones. Backed by an
/// *unbounded* channel so [`Self::emit`] never blocks or awaits — producers
/// emit from wherever they run without back-pressure coupling them to the
/// drain, mirroring how the synchronous paths hand a plain `Vec` of events back
/// to a non-blocking `broadcast`.
#[derive(Clone)]
pub struct AsyncEventSink {
    tx: mpsc::UnboundedSender<SessionEvent>,
}

/// The receiving half of the async event seam, drained by the server.
///
/// Handed to the transport layer, which pulls events off it in a background
/// task and forwards each to its broadcast. There is exactly one receiver per
/// channel.
pub type AsyncEventReceiver = mpsc::UnboundedReceiver<SessionEvent>;

impl AsyncEventSink {
    /// Create a fresh seam: the sink to hand the interactor and the receiver to
    /// hand the server's drain task.
    pub fn channel() -> (AsyncEventSink, AsyncEventReceiver) {
        let (tx, rx) = mpsc::unbounded_channel();
        (AsyncEventSink { tx }, rx)
    }

    /// Emit an event onto the seam.
    ///
    /// A send error means only that the receiver was dropped — the server's
    /// drain task is gone (shutdown), or no drain was wired at all. Dropping the
    /// event is the correct no-op then, exactly as [`AppState::broadcast`]
    /// ignores a send error when there are no subscribers.
    ///
    /// [`AppState::broadcast`]: https://docs.rs/delta-server
    pub fn emit(&self, event: SessionEvent) {
        let _ = self.tx.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delta_model::SessionId;

    #[tokio::test]
    async fn an_emitted_event_reaches_the_receiver() {
        let (sink, mut rx) = AsyncEventSink::channel();
        sink.emit(SessionEvent::SessionClosed {
            session_id: SessionId::from("sess-1"),
        });
        let received = rx.recv().await.expect("the emitted event is received");
        assert_eq!(
            received,
            SessionEvent::SessionClosed {
                session_id: SessionId::from("sess-1"),
            }
        );
    }

    #[tokio::test]
    async fn emitting_with_no_receiver_is_a_silent_no_op() {
        let (sink, rx) = AsyncEventSink::channel();
        drop(rx);
        // Must not panic: a dropped receiver is a benign "no drain" case.
        sink.emit(SessionEvent::SessionClosed {
            session_id: SessionId::from("sess-1"),
        });
    }
}
