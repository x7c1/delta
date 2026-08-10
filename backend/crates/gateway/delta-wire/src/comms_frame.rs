//! The wire form of one comms-log frame: the `/comms` stream contract.
//!
//! `/comms` is a per-session observability stream, deliberately separate from
//! the `/ws` conversation stream. A frame here is *not* a conversation event: it
//! is one message Delta exchanged with a headless provider's transport, mirrored
//! for a human to look at. Nothing on this stream is persisted, so a client that
//! misses a frame has missed it for good — which is why the server replays its
//! ring buffer before tailing live rather than pretending the stream is
//! complete.

use delta_usecase::{CommsDirection, CommsEntry, CommsFrameKind};
use serde::Serialize;
use ts_rs::TS;

/// Which way one frame travelled. Wire twin of [`CommsDirection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "CommsDirection")]
pub enum WireCommsDirection {
    /// Delta → agent: a frame Delta wrote.
    ToAgent,
    /// Agent → Delta: a frame Delta read.
    FromAgent,
}

impl From<CommsDirection> for WireCommsDirection {
    fn from(direction: CommsDirection) -> Self {
        match direction {
            CommsDirection::ToAgent => WireCommsDirection::ToAgent,
            CommsDirection::FromAgent => WireCommsDirection::FromAgent,
        }
    }
}

/// What kind of message one frame is. Wire twin of [`CommsFrameKind`].
///
/// A server-originated request is `from_agent` + `request`, not a fourth kind —
/// see the domain type for the full (direction, kind) table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "CommsFrameKind")]
pub enum WireCommsFrameKind {
    /// Carries an id and awaits a correlated response.
    Request,
    /// Answers a request, carrying its id.
    Response,
    /// Fire and forget: no id, no answer.
    Notification,
}

impl From<CommsFrameKind> for WireCommsFrameKind {
    fn from(kind: CommsFrameKind) -> Self {
        match kind {
            CommsFrameKind::Request => WireCommsFrameKind::Request,
            CommsFrameKind::Response => WireCommsFrameKind::Response,
            CommsFrameKind::Notification => WireCommsFrameKind::Notification,
        }
    }
}

/// One frame on the `/comms` stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "CommsFrame")]
pub struct WireCommsFrame {
    /// Per-session monotonic sequence number, minted by the server as the frame
    /// is recorded.
    ///
    /// The browser orders and de-duplicates on this rather than on the
    /// timestamp: two frames can share a millisecond, and the replay-then-tail
    /// handoff can only be made seamless by an ordering that is total.
    pub seq: u64,
    /// When the server recorded the frame, as Unix milliseconds.
    pub at_ms: i64,
    pub direction: WireCommsDirection,
    pub kind: WireCommsFrameKind,
    /// The method the frame names, for the kinds that name one. `null` on
    /// Delta's own answer to a server request (which names none).
    pub method: Option<String>,
    /// The frame as JSON **text**, not as a nested object.
    ///
    /// Deliberately a string: the payload is an opaque provider document the
    /// browser only ever displays (pretty-printed on expand), and keeping it a
    /// string means neither ts-rs nor the browser has to pretend it has a type.
    /// A payload that is somehow not parseable still renders as the text it is
    /// instead of breaking the frame it rides on.
    pub payload_json: String,
}

impl WireCommsFrame {
    /// Stamp a recorded entry with its ordering and project it onto the wire.
    ///
    /// `seq`/`at_ms` come from the server-side log (the sink owns ordering, see
    /// [`delta_usecase::CommsLogSink::record`]), so this conversion is the only
    /// place the two halves meet.
    pub fn new(seq: u64, at_ms: i64, entry: CommsEntry) -> Self {
        Self {
            seq,
            at_ms,
            direction: entry.direction.into(),
            kind: entry.kind.into(),
            method: entry.method,
            payload_json: entry.payload_json,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON envelope every client parses, pinned field for field.
    #[test]
    fn a_frame_serializes_with_snake_case_direction_and_kind() {
        let frame = WireCommsFrame::new(
            7,
            1_760_000_000_000,
            CommsEntry::new(
                CommsDirection::ToAgent,
                CommsFrameKind::Request,
                Some("turn/start"),
                r#"{"id":3,"method":"turn/start"}"#,
            ),
        );
        assert_eq!(
            serde_json::to_value(&frame).unwrap(),
            serde_json::json!({
                "seq": 7,
                "at_ms": 1_760_000_000_000_i64,
                "direction": "to_agent",
                "kind": "request",
                "method": "turn/start",
                "payload_json": r#"{"id":3,"method":"turn/start"}"#,
            }),
        );
    }

    /// A server-originated request is the `from_agent` + `request` pair, and a
    /// methodless answer serializes its `method` as `null` rather than omitting
    /// it — the browser's type says the field is always present.
    #[test]
    fn a_server_request_and_a_methodless_answer_project_as_expected() {
        let server_request = WireCommsFrame::new(
            1,
            0,
            CommsEntry::new(
                CommsDirection::FromAgent,
                CommsFrameKind::Request,
                Some("item/fileChange/requestApproval"),
                "{}",
            ),
        );
        let value = serde_json::to_value(&server_request).unwrap();
        assert_eq!(value["direction"], "from_agent");
        assert_eq!(value["kind"], "request");

        let answer = WireCommsFrame::new(
            2,
            0,
            CommsEntry::new(
                CommsDirection::ToAgent,
                CommsFrameKind::Response,
                None,
                r#"{"id":"srv-1","result":{}}"#,
            ),
        );
        let value = serde_json::to_value(&answer).unwrap();
        assert!(value.get("method").is_some(), "the key is always present");
        assert_eq!(value["method"], serde_json::Value::Null);
    }
}
