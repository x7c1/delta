//! The `codex app-server` wire framing: newline-delimited JSON-RPC 2.0.
//!
//! Each frame is a single JSON object on its own line (`\n`-terminated). The
//! shapes are JSON-RPC 2.0 minus the `"jsonrpc": "2.0"` version tag, which the
//! app-server omits on the wire — so outgoing frames are serialised without it
//! and incoming frames are parsed leniently (any `jsonrpc` field, and any other
//! unknown field, is ignored).
//!
//! Three message directions matter:
//!
//! - **client → server request**: `{ "id", "method", "params"? }` — awaits a
//!   correlated response.
//! - **client → server notification**: `{ "method", "params"? }` — fire and
//!   forget, no id.
//! - **server → client**: a [`Response`] to one of our requests
//!   (`{ "id", "result" | "error" }`), a server-originated [`ServerRequest`]
//!   (`{ "id", "method", "params"? }`, e.g. `*/requestApproval`), or a
//!   [`Notification`] (`{ "method", "params"? }`, e.g. `item/*` / `turn/*`).
//!
//! This module is the single place the byte-level contract lives, so pinning it
//! against the vendored app-server schema later touches only this module.
//!
//! **Inferred, not yet verified against a vendored schema:** that the version
//! tag is omitted, that request ids are integers, and that thread-scoped
//! notifications carry their `threadId` under `params.threadId`. The parser is
//! deliberately lenient so a later correction is localised here.

use serde::Deserialize;
use serde_json::{json, Value};

mod incoming;
pub use incoming::Incoming;

mod notification;
pub use notification::Notification;

mod outgoing_notification;
pub use outgoing_notification::OutgoingNotification;

mod outgoing_request;
pub use outgoing_request::OutgoingRequest;

mod parse_error;
pub use parse_error::ParseError;

mod response;
pub use response::Response;

mod rpc_error;
pub use rpc_error::RpcError;

mod server_request;
pub use server_request::ServerRequest;

/// A JSON-RPC request id.
///
/// Delta originates request ids as a monotonic integer counter. Ids on
/// server-originated requests are preserved as raw JSON (see
/// [`ServerRequest::id`]) and echoed back verbatim when responding, so this
/// integer type only names the ids Delta mints.
pub type RequestId = i64;

/// The JSON key thread-scoped messages carry their thread id under.
const THREAD_ID_KEY: &str = "threadId";

/// The lenient on-the-wire shape every incoming frame is first parsed into,
/// before [`classify`] decides what it is. Unknown fields (including any
/// `jsonrpc` tag) are ignored.
#[derive(Debug, Deserialize)]
struct RawFrame {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<RpcError>,
}

/// Parse one newline-delimited frame into a classified [`Incoming`].
pub fn parse_incoming(line: &str) -> std::result::Result<Incoming, ParseError> {
    let frame: RawFrame = serde_json::from_str(line)?;
    classify(frame)
}

fn classify(frame: RawFrame) -> std::result::Result<Incoming, ParseError> {
    match (frame.method, frame.id) {
        // A method with an id is a server-originated request.
        (Some(method), Some(id)) => Ok(Incoming::ServerRequest(ServerRequest {
            id,
            method,
            params: frame.params.unwrap_or(Value::Null),
        })),
        // A method without an id is a notification.
        (Some(method), None) => Ok(Incoming::Notification(Notification {
            method,
            params: frame.params.unwrap_or(Value::Null),
        })),
        // No method but an id is a response to one of our requests.
        (None, Some(id)) => {
            let id = id.as_i64().ok_or_else(|| {
                ParseError::NotRpc(format!("response id is not an integer: {id}"))
            })?;
            let outcome = match (frame.result, frame.error) {
                (_, Some(error)) => Err(error),
                (Some(result), None) => Ok(result),
                // A response with neither result nor error: treat as a success
                // carrying a null payload rather than rejecting the frame.
                (None, None) => Ok(Value::Null),
            };
            Ok(Incoming::Response(Response { id, outcome }))
        }
        (None, None) => Err(ParseError::NotRpc(
            "frame carries neither a method nor an id".to_owned(),
        )),
    }
}

/// Serialise an outgoing request as a single newline-terminated frame.
pub fn encode_request(
    id: RequestId,
    method: &str,
    params: Option<Value>,
) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(&OutgoingRequest { id, method, params })?;
    line.push('\n');
    Ok(line)
}

/// Serialise an outgoing notification as a single newline-terminated frame.
pub fn encode_notification(method: &str, params: Option<Value>) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(&OutgoingNotification { method, params })?;
    line.push('\n');
    Ok(line)
}

/// Serialise a success response to a server-originated request. `id` is echoed
/// back verbatim (the server's own id type), so a string id stays a string and
/// an integer id stays an integer.
pub fn encode_success_response(id: &Value, result: Value) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(&json!({ "id": id, "result": result }))?;
    line.push('\n');
    Ok(line)
}

/// Serialise an error response to a server-originated request. Used to answer a
/// request Delta does not model so the server does not block waiting on a reply.
pub fn encode_error_response(id: &Value, code: i64, message: &str) -> serde_json::Result<String> {
    let mut line =
        serde_json::to_string(&json!({ "id": id, "error": { "code": code, "message": message } }))?;
    line.push('\n');
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn outgoing_request_omits_the_jsonrpc_tag_and_absent_params() {
        let line = encode_request(7, "thread/start", None).unwrap();
        assert!(line.ends_with('\n'));
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value, json!({ "id": 7, "method": "thread/start" }));
        assert!(value.get("jsonrpc").is_none());
        assert!(value.get("params").is_none());
    }

    #[test]
    fn outgoing_request_includes_params_when_present() {
        let line = encode_request(1, "turn/start", Some(json!({ "threadId": "thr_1" }))).unwrap();
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            value,
            json!({ "id": 1, "method": "turn/start", "params": { "threadId": "thr_1" } })
        );
    }

    #[test]
    fn outgoing_notification_has_no_id() {
        let line = encode_notification("initialized", None).unwrap();
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value, json!({ "method": "initialized" }));
        assert!(value.get("id").is_none());
    }

    #[test]
    fn classifies_a_success_response() {
        let msg = parse_incoming(r#"{"id":3,"result":{"threadId":"thr_9"}}"#).unwrap();
        assert_eq!(
            msg,
            Incoming::Response(Response {
                id: 3,
                outcome: Ok(json!({ "threadId": "thr_9" })),
            })
        );
        assert_eq!(msg.thread_id(), None, "responses are correlated by id");
    }

    #[test]
    fn classifies_an_error_response() {
        let msg = parse_incoming(r#"{"id":4,"error":{"code":-32601,"message":"no"}}"#).unwrap();
        assert_eq!(
            msg,
            Incoming::Response(Response {
                id: 4,
                outcome: Err(RpcError {
                    code: -32601,
                    message: "no".to_owned(),
                    data: None,
                }),
            })
        );
    }

    #[test]
    fn tolerates_an_explicit_jsonrpc_tag_on_incoming() {
        // A future/real server that DOES include the version tag must still parse.
        let msg = parse_incoming(r#"{"jsonrpc":"2.0","id":5,"result":null}"#).unwrap();
        assert_eq!(
            msg,
            Incoming::Response(Response {
                id: 5,
                outcome: Ok(Value::Null),
            })
        );
    }

    #[test]
    fn classifies_a_notification_and_extracts_its_thread_id() {
        let msg = parse_incoming(
            r#"{"method":"item/completed","params":{"threadId":"thr_2","item":{}}}"#,
        )
        .unwrap();
        match &msg {
            Incoming::Notification(n) => assert_eq!(n.method, "item/completed"),
            other => panic!("expected notification, got {other:?}"),
        }
        assert_eq!(msg.thread_id(), Some("thr_2"));
    }

    #[test]
    fn classifies_a_server_request() {
        let msg = parse_incoming(
            r#"{"id":"srv-1","method":"item/commandExecution/requestApproval","params":{"threadId":"thr_3"}}"#,
        )
        .unwrap();
        match &msg {
            Incoming::ServerRequest(r) => {
                assert_eq!(r.method, "item/commandExecution/requestApproval");
                assert_eq!(r.id, json!("srv-1"), "server ids are preserved verbatim");
            }
            other => panic!("expected server request, got {other:?}"),
        }
        assert_eq!(msg.thread_id(), Some("thr_3"));
    }

    #[test]
    fn a_notification_without_a_thread_id_extracts_none() {
        let msg = parse_incoming(r#"{"method":"server/status","params":{"ok":true}}"#).unwrap();
        assert_eq!(msg.thread_id(), None);
    }

    #[test]
    fn rejects_a_frame_with_neither_method_nor_id() {
        let err = parse_incoming(r#"{"params":{}}"#).unwrap_err();
        assert!(matches!(err, ParseError::NotRpc(_)));
    }

    /// Every incoming shape re-renders as the frame it was parsed from, which is
    /// what the comms-log inspector displays. Pinned per shape so a change to
    /// the parsed types cannot silently change what the inspector shows.
    #[test]
    fn incoming_frames_re_render_as_their_json() {
        let response = parse_incoming(r#"{"id":3,"result":{"threadId":"thr_9"}}"#).unwrap();
        let Incoming::Response(response) = response else {
            panic!("expected a response");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&response.to_frame_json()).unwrap(),
            json!({ "id": 3, "result": { "threadId": "thr_9" } }),
        );

        let error = parse_incoming(r#"{"id":4,"error":{"code":-32601,"message":"no"}}"#).unwrap();
        let Incoming::Response(error) = error else {
            panic!("expected a response");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&error.to_frame_json()).unwrap(),
            json!({ "id": 4, "error": { "code": -32601, "message": "no" } }),
        );

        let notification =
            parse_incoming(r#"{"method":"item/completed","params":{"threadId":"thr_2"}}"#).unwrap();
        let Incoming::Notification(notification) = notification else {
            panic!("expected a notification");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&notification.to_frame_json()).unwrap(),
            json!({ "method": "item/completed", "params": { "threadId": "thr_2" } }),
        );

        let request =
            parse_incoming(r#"{"id":"srv-1","method":"item/x/requestApproval","params":{}}"#)
                .unwrap();
        let Incoming::ServerRequest(request) = request else {
            panic!("expected a server request");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&request.to_frame_json()).unwrap(),
            json!({ "id": "srv-1", "method": "item/x/requestApproval", "params": {} }),
        );
    }

    #[test]
    fn rejects_non_json() {
        assert!(matches!(
            parse_incoming("not json").unwrap_err(),
            ParseError::Json(_)
        ));
    }
}
