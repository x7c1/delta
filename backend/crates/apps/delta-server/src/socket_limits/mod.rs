//! Bounds on the three live sockets: how large an inbound message may be, and
//! how many sockets may be open at once.
//!
//! The sockets sit behind the bearer token, so these are not a defense against
//! the outside world; they keep one token holder (or one runaway tab) from
//! exhausting the server's memory with huge frames or an unbounded number of
//! open sockets. axum's defaults (64 MiB messages, 16 MiB frames, no socket
//! cap) are sized for no channel in particular, so each channel names its own:
//!
//! - **Message size** — the events (`/ws`) and comms (`/comms`) streams are
//!   server → client: a client only ever sends control frames on them, so they
//!   accept almost nothing inbound ([`IDLE_INBOUND_LIMIT`]). The terminal bridge
//!   (`/pty`) carries keystrokes and pastes, so it allows a generous paste
//!   ([`PTY_INBOUND_LIMIT`]). Both the frame and the message bound are set, so a
//!   too-large frame is refused from its header — before its payload is read —
//!   and a message assembled from many small fragments is refused as soon as it
//!   passes the bound. Either way the socket is closed.
//! - **Concurrent sockets** — `/pty` and `/comms` are per session, so each
//!   session may hold [`SESSION_SOCKETS_CAP`] of each; `/ws` is process-wide, so
//!   it is capped globally at [`EVENT_SOCKETS_CAP`]. An upgrade over its cap is
//!   refused with `429 Too Many Requests` and a `warn` log naming the endpoint
//!   and the cap. A slot is a [`SocketSlot`] the socket's task owns, released
//!   when it is dropped, so a socket that ends any way at all — a normal close,
//!   a receive error, a panic in its pump, or an upgrade that never completes —
//!   gives its slot back.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use axum::extract::ws::WebSocketUpgrade;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// The largest inbound message (and frame) the events and comms streams accept.
///
/// Neither stream carries client → server data: a client sends only close,
/// ping and pong frames, whose payload the protocol caps at 125 bytes. 4 KiB
/// leaves room for any well-behaved client while refusing anything that could
/// make the server buffer real data.
pub(crate) const IDLE_INBOUND_LIMIT: usize = 4 * 1024;

/// The largest inbound message (and frame) the terminal bridge accepts.
///
/// The bridge carries keystrokes (a few bytes each), resize control messages,
/// and pastes, which xterm.js sends as one message. 1 MiB is far above any
/// paste a person makes into a terminal, yet small enough that a hostile
/// client cannot make the server hold a large buffer per socket.
pub(crate) const PTY_INBOUND_LIMIT: usize = 1024 * 1024;

/// How many `/pty` sockets — and, separately, how many `/comms` sockets — one
/// session may hold open at once.
///
/// A pane is watched by one socket per browser tab showing it; 8 covers a
/// session open in several tabs or windows, plus reconnects whose old socket
/// has not finished closing.
pub(crate) const SESSION_SOCKETS_CAP: usize = 8;

/// How many `/ws` event sockets may be open at once, across all clients.
///
/// Each browser tab holds exactly one, for its whole lifetime, so 32 covers
/// far more tabs than one person keeps open while bounding the broadcast
/// subscribers (each of which buffers events until its socket drains).
pub(crate) const EVENT_SOCKETS_CAP: usize = 32;

/// The connection pools of the three live sockets, shared through the state.
pub(crate) struct SocketLimits {
    events: Arc<SlotPool>,
    comms: Arc<SlotPool>,
    pty: Arc<SlotPool>,
}

impl SocketLimits {
    /// The pools at their documented caps.
    pub(crate) fn new() -> Self {
        Self {
            events: SlotPool::new("/ws", EVENT_SOCKETS_CAP),
            comms: SlotPool::new("/comms", SESSION_SOCKETS_CAP),
            pty: SlotPool::new("/pty", SESSION_SOCKETS_CAP),
        }
    }

    /// A slot on the process-wide event stream.
    pub(crate) fn acquire_events(&self) -> Result<SocketSlot, SocketsExhausted> {
        self.events.try_acquire("")
    }

    /// A slot on `session_id`'s comms-log stream.
    pub(crate) fn acquire_comms(&self, session_id: &str) -> Result<SocketSlot, SocketsExhausted> {
        self.comms.try_acquire(session_id)
    }

    /// A slot on `session_id`'s terminal bridge.
    pub(crate) fn acquire_pty(&self, session_id: &str) -> Result<SocketSlot, SocketsExhausted> {
        self.pty.try_acquire(session_id)
    }
}

/// `upgrade` with both its message and frame size bounded by `limit`.
pub(crate) fn bound_inbound(upgrade: WebSocketUpgrade, limit: usize) -> WebSocketUpgrade {
    upgrade.max_message_size(limit).max_frame_size(limit)
}

/// The open sockets of one endpoint, counted per key (a session id, or the one
/// empty key of a process-wide pool) against a shared cap.
struct SlotPool {
    endpoint: &'static str,
    cap: usize,
    /// Open sockets per key. A key whose count drops to zero is removed, so the
    /// map is bounded by the keys with a socket open, not by every key ever
    /// asked for.
    held: Mutex<HashMap<String, usize>>,
}

impl SlotPool {
    fn new(endpoint: &'static str, cap: usize) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
            cap,
            held: Mutex::new(HashMap::new()),
        })
    }

    fn try_acquire(self: &Arc<Self>, key: &str) -> Result<SocketSlot, SocketsExhausted> {
        // The counts are plain integers updated in one step, so a panic while
        // the lock is held cannot leave them half-written; a poisoned lock is
        // used as it stands rather than wedging every later upgrade.
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let count = held.entry(key.to_owned()).or_insert(0);
        if *count >= self.cap {
            tracing::warn!(
                endpoint = self.endpoint,
                cap = self.cap,
                "refusing a socket upgrade: the endpoint's connection cap is reached"
            );
            return Err(SocketsExhausted {
                endpoint: self.endpoint,
                cap: self.cap,
            });
        }
        *count += 1;
        Ok(SocketSlot {
            pool: Arc::clone(self),
            key: key.to_owned(),
        })
    }

    fn release(&self, key: &str) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = held.get_mut(key) {
            *count -= 1;
            if *count == 0 {
                held.remove(key);
            }
        }
    }

    #[cfg(test)]
    fn open(&self, key: &str) -> usize {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .copied()
            .unwrap_or(0)
    }
}

/// One open socket's place in its endpoint's pool, given back on drop.
///
/// Moved into the socket's task, so every way that task ends — including a
/// panic, which drops the task's future while unwinding — releases the slot.
pub(crate) struct SocketSlot {
    pool: Arc<SlotPool>,
    key: String,
}

impl Drop for SocketSlot {
    fn drop(&mut self) {
        self.pool.release(&self.key);
    }
}

/// An upgrade refused because its endpoint's connection cap is reached.
#[derive(Debug)]
pub(crate) struct SocketsExhausted {
    endpoint: &'static str,
    cap: usize,
}

impl IntoResponse for SocketsExhausted {
    fn into_response(self) -> Response {
        (
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "too many open {} sockets (at most {} at once)",
                self.endpoint, self.cap
            ),
        )
            .into_response()
    }
}

#[cfg(test)]
pub(crate) mod test_client;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_refused_at_its_cap_and_accepted_again_once_a_slot_is_dropped() {
        let pool = SlotPool::new("/test", 2);
        let first = pool.try_acquire("a").unwrap();
        let _second = pool.try_acquire("a").unwrap();

        let Err(refused) = pool.try_acquire("a") else {
            panic!("a third socket is accepted under a cap of two");
        };
        assert_eq!(
            refused.into_response().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        // The cap is per key: another session is unaffected.
        let _other = pool.try_acquire("b").unwrap();

        drop(first);
        assert!(pool.try_acquire("a").is_ok());
    }

    #[test]
    fn a_key_with_no_open_slot_is_forgotten() {
        let pool = SlotPool::new("/test", 1);
        drop(pool.try_acquire("a").unwrap());
        assert!(pool
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty());
    }

    /// A pump that panics drops its task's future while unwinding, and with it
    /// the slot it owns.
    #[tokio::test]
    async fn a_panicking_socket_task_releases_its_slot() {
        let pool = SlotPool::new("/test", 1);
        let slot = pool.try_acquire("a").unwrap();
        let task = tokio::spawn(async move {
            let _slot = slot;
            panic!("the pump crashed");
        });
        assert!(task.await.unwrap_err().is_panic());
        assert_eq!(pool.open("a"), 0);
    }
}
