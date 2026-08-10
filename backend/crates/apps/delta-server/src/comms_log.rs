//! The in-memory comms log: where an adapter's frames are buffered and fanned
//! out to the browsers watching a session.
//!
//! This is the transport layer's implementation of
//! [`CommsLogSink`](delta_usecase::CommsLogSink) — the only place a recorded
//! frame is ever held. Nothing here reaches the database, the conversation event
//! stream, or attribution: the log is a **live view**, and a server restart
//! starts it empty by design (a deliberate v1 trade: these frames are not worth
//! the schema and write cost of persisting them).
//!
//! ## Shape
//!
//! One bounded ring buffer plus one broadcast channel per live session:
//!
//! - the **ring** is what a browser connecting mid-session replays, so opening
//!   the pane during a turn shows the frames that already flew rather than an
//!   empty box that fills only on the next one. It keeps the most recent
//!   [`COMMS_RING_CAPACITY`] frames and drops the oldest — a long session's
//!   history is unbounded, the interesting part is always the recent part;
//! - the **broadcast** is the live tail every subscriber shares.
//!
//! ## Why nothing here can block the agent
//!
//! [`CommsLogSink::record`] runs on the adapter's own send/receive path, so a
//! slow or absent browser must never be able to hold a turn up. Two properties
//! guarantee that, and both are structural rather than best-effort:
//!
//! - the ring is bounded and evicts the oldest frame, so recording never grows
//!   memory without limit and never waits for a reader;
//! - [`tokio::sync::broadcast`] is a *lossy* fan-out: `send` returns
//!   immediately whether or not anyone is listening, and a receiver that falls
//!   [`COMMS_BROADCAST_CAPACITY`] frames behind is told it lagged instead of
//!   applying back-pressure to the sender.
//!
//! The only lock taken is a `std::sync::Mutex` held for a push and a
//! non-awaiting send — no `.await` happens while it is held, so it cannot be a
//! scheduling hazard either.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

use delta_usecase::{CommsEntry, CommsLogSink};
use delta_wire::WireCommsFrame;

/// How many recent frames one session's replay buffer holds.
///
/// Sized to cover "what just happened" — a Codex turn's `turn/*` + `item/*`
/// flow runs to a few dozen frames, so a few hundred spans several turns while
/// staying trivially small in memory (each frame is one JSON line).
pub const COMMS_RING_CAPACITY: usize = 400;

/// How far behind a live subscriber may fall before it is told it lagged.
///
/// Generous relative to the frame rate of a single session, but bounded: the
/// bound is what keeps a stalled browser from ever slowing the agent down.
const COMMS_BROADCAST_CAPACITY: usize = 256;

/// One live session's log: its replay ring, its live fan-out, and the sequence
/// counter that orders both.
struct SessionLog {
    /// The next sequence number to mint. Per session, so a session's frames are
    /// numbered from its own start rather than from a process-wide counter.
    next_seq: u64,
    /// The most recent frames, oldest first, capped at [`COMMS_RING_CAPACITY`].
    ring: VecDeque<WireCommsFrame>,
    /// The live tail. Kept even with no receivers so a later subscriber joins the
    /// same stream.
    live: broadcast::Sender<WireCommsFrame>,
}

impl SessionLog {
    fn new() -> Self {
        let (live, _) = broadcast::channel(COMMS_BROADCAST_CAPACITY);
        Self {
            next_seq: 0,
            ring: VecDeque::with_capacity(COMMS_RING_CAPACITY),
            live,
        }
    }
}

/// The process-wide comms log: per-session buffers, keyed by Delta session id.
///
/// Cheap to share behind an `Arc`: the composition root hands one out as the
/// [`CommsLogSink`] every adapter records into, and the `/comms` route reads the
/// same instance.
pub struct CommsLogHub {
    sessions: Mutex<HashMap<String, SessionLog>>,
}

impl Default for CommsLogHub {
    fn default() -> Self {
        Self::new()
    }
}

impl CommsLogHub {
    /// An empty log.
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Watch one session's log: its buffered frames first, then the live tail.
    ///
    /// The snapshot and the live subscription are taken under one lock, which is
    /// what makes the handoff seamless in both directions — a frame recorded in
    /// between cannot slip past the snapshot (it lands on the already-created
    /// subscription) and cannot arrive twice (the subscription starts after the
    /// frames the snapshot copied).
    ///
    /// Subscribing to a session with no frames yet creates its buffer, so a pane
    /// opened before the first frame still tails live rather than having to
    /// reconnect. A buffer created that way but never written to is reclaimed by a
    /// later subscribe (see the pruning below), so the guarantee costs no
    /// permanent memory.
    pub fn subscribe(&self, session_id: &str) -> CommsSubscription {
        let mut sessions = self.sessions.lock().expect("comms log mutex poisoned");
        // Drop any buffer that is both empty and unwatched: such an entry can only
        // be a leftover from a subscription naming a session no adapter ever drove
        // (`discard` releases every other kind), since a live session's buffer
        // holds frames and a watched one has a receiver. Pruning here bounds the
        // map by concurrent watchers rather than by ids ever asked for.
        sessions.retain(|_, log| !log.ring.is_empty() || log.live.receiver_count() > 0);
        let log = sessions
            .entry(session_id.to_owned())
            .or_insert_with(SessionLog::new);
        CommsSubscription {
            replay: log.ring.iter().cloned().collect(),
            live: log.live.subscribe(),
        }
    }

    /// How many frames a session currently has buffered. Test/diagnostic view of
    /// the ring's bound.
    #[cfg(test)]
    fn buffered(&self, session_id: &str) -> usize {
        self.sessions
            .lock()
            .expect("comms log mutex poisoned")
            .get(session_id)
            .map_or(0, |log| log.ring.len())
    }
}

impl CommsLogSink for CommsLogHub {
    /// Stamp `entry` with its ordering, buffer it, and fan it out — without ever
    /// awaiting or waiting on a consumer (see the module docs).
    fn record(&self, session_id: &str, entry: CommsEntry) {
        let mut sessions = self.sessions.lock().expect("comms log mutex poisoned");
        let log = sessions
            .entry(session_id.to_owned())
            .or_insert_with(SessionLog::new);
        let seq = log.next_seq;
        log.next_seq += 1;
        let frame = WireCommsFrame::new(seq, now_ms(), entry);
        if log.ring.len() == COMMS_RING_CAPACITY {
            log.ring.pop_front();
        }
        log.ring.push_back(frame.clone());
        // A send error only means nobody is watching right now, which is the
        // normal case: the log exists to be there when someone looks.
        let _ = log.live.send(frame);
    }

    fn discard(&self, session_id: &str) {
        // Dropping the entry drops its broadcast sender, which closes every live
        // subscriber's stream — the socket then ends rather than hanging on a
        // session that no longer exists.
        self.sessions
            .lock()
            .expect("comms log mutex poisoned")
            .remove(session_id);
    }
}

/// One watcher's ordered view of a session's log: the replay, then the tail.
///
/// Deliberately a type of its own rather than a bare receiver, so the
/// replay-then-tail contract is expressed once and testable without a WebSocket
/// (the `/comms` handler does nothing but pump this into a socket).
pub struct CommsSubscription {
    /// The buffered frames still to be delivered, oldest first.
    replay: VecDeque<WireCommsFrame>,
    /// The live tail, joined at the instant the replay was snapshotted.
    live: broadcast::Receiver<WireCommsFrame>,
}

impl CommsSubscription {
    /// The next frame to show, or `None` once the session's log is gone (the
    /// session ended, or the server is shutting down).
    ///
    /// A subscriber that fell behind the broadcast bound is skipped forward with
    /// a warning rather than closed: a gap in an observability stream is
    /// survivable, and closing would lose the frames still coming.
    pub async fn next(&mut self) -> Option<WireCommsFrame> {
        if let Some(frame) = self.replay.pop_front() {
            return Some(frame);
        }
        loop {
            match self.live.recv().await {
                Ok(frame) => return Some(frame),
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "comms log subscriber lagged");
                }
                Err(RecvError::Closed) => return None,
            }
        }
    }
}

/// Now, as Unix milliseconds. A clock before the epoch is not a case worth
/// carrying an error for, so it reads as 0.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_usecase::{CommsDirection, CommsFrameKind};
    use delta_wire::{WireCommsDirection, WireCommsFrameKind};

    /// Record one outbound request against `session_id`, named `method`.
    fn record_request(hub: &CommsLogHub, session_id: &str, method: &str) {
        hub.record(
            session_id,
            CommsEntry::new(
                CommsDirection::ToAgent,
                CommsFrameKind::Request,
                Some(method),
                format!(r#"{{"method":"{method}"}}"#),
            ),
        );
    }

    /// The endpoint's contract: a watcher joining mid-session receives the frames
    /// that already flew, in order, and then the next live one on the same
    /// stream. This is what makes opening the pane during a turn useful rather
    /// than showing an empty box.
    #[tokio::test]
    async fn a_mid_session_subscriber_replays_the_buffer_then_tails_live() {
        let hub = CommsLogHub::new();
        record_request(&hub, "sess-1", "thread/start");
        record_request(&hub, "sess-1", "turn/start");

        let mut watcher = hub.subscribe("sess-1");

        let first = watcher.next().await.expect("a replayed frame");
        assert_eq!(first.seq, 0);
        assert_eq!(first.method.as_deref(), Some("thread/start"));
        assert_eq!(first.direction, WireCommsDirection::ToAgent);
        assert_eq!(first.kind, WireCommsFrameKind::Request);

        let second = watcher.next().await.expect("the second replayed frame");
        assert_eq!(second.seq, 1);
        assert_eq!(second.method.as_deref(), Some("turn/start"));

        // Now live: a frame recorded after the subscription arrives on the same
        // stream, numbered after the replayed ones — no gap, no repeat.
        record_request(&hub, "sess-1", "turn/interrupt");
        let live = watcher.next().await.expect("a live frame");
        assert_eq!(live.seq, 2);
        assert_eq!(live.method.as_deref(), Some("turn/interrupt"));
    }

    /// Frames are per session: one session's watcher never sees another's, so a
    /// shared app-server hosting several threads does not leak one session's
    /// wire into another's inspector.
    #[tokio::test]
    async fn sessions_do_not_see_each_others_frames() {
        let hub = CommsLogHub::new();
        record_request(&hub, "sess-1", "turn/start");
        record_request(&hub, "sess-2", "thread/inject_items");

        let mut watcher = hub.subscribe("sess-1");
        let frame = watcher.next().await.expect("its own frame");
        assert_eq!(frame.method.as_deref(), Some("turn/start"));
        assert_eq!(frame.seq, 0, "each session numbers from its own start");
        assert_eq!(hub.buffered("sess-2"), 1);
    }

    /// The ring is bounded and drops the oldest: recording far past the capacity
    /// neither grows without limit nor blocks, and the replay keeps the recent
    /// end (which is the end anyone looking at a live session wants).
    #[tokio::test]
    async fn the_ring_keeps_the_most_recent_frames_and_no_more() {
        let hub = CommsLogHub::new();
        let overflow = COMMS_RING_CAPACITY + 50;
        for i in 0..overflow {
            record_request(&hub, "sess-1", &format!("method/{i}"));
        }
        assert_eq!(hub.buffered("sess-1"), COMMS_RING_CAPACITY);

        let mut watcher = hub.subscribe("sess-1");
        let oldest = watcher.next().await.expect("the oldest surviving frame");
        assert_eq!(
            oldest.seq,
            (overflow - COMMS_RING_CAPACITY) as u64,
            "the front of the ring is the oldest frame still held"
        );
    }

    /// The non-blocking invariant, from the sink's side: recording with **no**
    /// consumer attached and with the broadcast bound long since exceeded still
    /// completes — the sink can neither wait for a reader nor refuse a frame, so
    /// it cannot stall the adapter that is calling it.
    #[tokio::test]
    async fn recording_completes_with_no_consumer_and_a_saturated_channel() {
        let hub = CommsLogHub::new();
        // A subscriber that never reads: its broadcast slot fills at
        // COMMS_BROADCAST_CAPACITY and stays full.
        let _stalled = hub.subscribe("sess-1");
        for i in 0..(COMMS_BROADCAST_CAPACITY * 3) {
            record_request(&hub, "sess-1", &format!("method/{i}"));
        }
        // Reached here without awaiting anything: every record returned at once,
        // and the ring is still at its bound rather than having grown.
        assert_eq!(hub.buffered("sess-1"), COMMS_RING_CAPACITY);
    }

    /// Discarding a session ends its watchers' streams instead of leaving them
    /// hanging on a session that no longer exists, and releases its buffer.
    #[tokio::test]
    async fn discarding_a_session_closes_its_watchers() {
        let hub = CommsLogHub::new();
        record_request(&hub, "sess-1", "turn/start");
        let mut watcher = hub.subscribe("sess-1");
        assert!(watcher.next().await.is_some(), "the replayed frame");

        hub.discard("sess-1");
        assert_eq!(hub.buffered("sess-1"), 0);
        assert!(
            watcher.next().await.is_none(),
            "the stream ends when the session's log is gone"
        );
    }

    /// A subscription for a session that never records anything does not leave a
    /// buffer behind once its watcher is gone: the next subscribe prunes it, so a
    /// client asking for arbitrary session ids cannot grow the map without bound.
    #[tokio::test]
    async fn an_empty_unwatched_buffer_is_reclaimed() {
        let hub = CommsLogHub::new();
        {
            let _watcher = hub.subscribe("sess-never-recorded");
            assert!(
                hub.sessions
                    .lock()
                    .unwrap()
                    .contains_key("sess-never-recorded"),
                "the subscription created its buffer, so no first frame can be missed"
            );
        }
        // The watcher is gone and the buffer never held a frame.
        hub.subscribe("sess-other");
        assert!(
            !hub.sessions
                .lock()
                .unwrap()
                .contains_key("sess-never-recorded"),
            "the leftover empty buffer was reclaimed"
        );
    }

    /// Pruning must never touch a session that has frames, even while nobody is
    /// watching it — that buffer IS the replay a pane opened later depends on.
    #[tokio::test]
    async fn a_buffer_with_frames_survives_pruning_while_unwatched() {
        let hub = CommsLogHub::new();
        record_request(&hub, "sess-1", "turn/start");
        hub.subscribe("sess-2");

        let mut watcher = hub.subscribe("sess-1");
        let frame = watcher.next().await.expect("the buffered frame survived");
        assert_eq!(frame.method.as_deref(), Some("turn/start"));
    }

    /// A watcher that subscribes before the first frame is recorded still sees
    /// it: the buffer is created by the subscription, so there is no window in
    /// which frames are dropped for want of a session entry.
    #[tokio::test]
    async fn subscribing_before_the_first_frame_still_tails_it() {
        let hub = CommsLogHub::new();
        let mut watcher = hub.subscribe("sess-1");
        record_request(&hub, "sess-1", "thread/start");
        let frame = watcher.next().await.expect("the first frame");
        assert_eq!(frame.method.as_deref(), Some("thread/start"));
        assert_eq!(frame.seq, 0);
    }
}
