//! The scenario script: what the fake app-server does, in reaction to the
//! client's requests.
//!
//! A scenario is a JSON file:
//!
//! ```json
//! {
//!   "server_info": { "name": "fake-codex", "version": "0" },
//!   "thread_id": "thr_fake_0001",
//!   "turn": {
//!     "turn_id": "turn_fake_0001",
//!     "emit": [
//!       { "type": "item_started",   "item": { "id": "item_1", "itemType": "agent_message" } },
//!       { "type": "item_completed", "item": { "id": "item_1", "itemType": "agent_message", "text": "hi" } },
//!       { "type": "request_approval", "method": "item/requestApproval", "params": { "toolName": "Bash" } },
//!       { "type": "turn_completed", "status": "completed" }
//!     ]
//!   }
//! }
//! ```
//!
//! Top-level fields:
//!
//! - `server_info` (optional): the object returned as `initialize` →
//!   `result.serverInfo`.
//! - `thread_id` (default `"thr_fake_0001"`): the id returned from
//!   `thread/start` and stamped into every emitted notification's
//!   `params.threadId` (unless the `turn/start` request itself named a thread).
//! - `turn` (optional): what a `turn/start` request plays. When absent, a
//!   `turn/start` still gets a response but emits nothing.
//!
//! A `turn`'s `emit` list plays strictly in order once the `turn/start`
//! response is written. The step vocabulary:
//!
//! | step | effect |
//! |---|---|
//! | `item_started { item }` | Emit an `item/started` notification carrying `item`. |
//! | `item_completed { item }` | Emit an `item/completed` notification carrying `item`. |
//! | `turn_started` | Emit a `turn/started` notification. |
//! | `turn_completed { status }` | Emit a `turn/completed` notification carrying `status` (e.g. `completed`, `interrupted`, `failed`). |
//! | `request_approval { method?, params?, blocking? }` | Emit a server → client request (default method `item/requestApproval`) with a freshly minted id. With `blocking: false` (default) the fake emits and continues. With `blocking: true` the fake **suspends** the turn after emitting it and resumes only once the client answers; on resuming it echoes the received `accept`/`decline` as an assistant message before playing the rest of the turn. |
//! | `notification { method, params? }` | Emit an arbitrary notification (escape hatch for shapes not covered above). |
//!
//! Every emitted notification (and the approval request) gets `threadId` stamped
//! into its `params` so the client transport can demux it to the right thread.
//!
//! A separate `turn/interrupt` request (whenever it arrives) must carry
//! `{threadId, turnId}` (the fake rejects it with a JSON-RPC error if the
//! `turnId` is missing); it is answered and followed by a `turn/completed`
//! notification with status `interrupted`, echoing the interrupted turn's id.
//!
//! How the file is found:
//!
//! 1. `FAKE_CODEX_SCENARIO` — an explicit path.
//! 2. Unset — a built-in default (one thread, one short assistant-message turn)
//!    so a manually launched fake holds a plausible conversation.

use serde::Deserialize;
use serde_json::{json, Value};

/// The default method for a scripted server → client approval request.
pub const DEFAULT_APPROVAL_METHOD: &str = "item/requestApproval";
/// The default thread id when a scenario does not name one.
pub const DEFAULT_THREAD_ID: &str = "thr_fake_0001";

/// One scripted emission played during a turn. See the module docs.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Emit {
    ItemStarted {
        item: Value,
    },
    ItemCompleted {
        item: Value,
    },
    TurnStarted,
    TurnCompleted {
        status: String,
    },
    RequestApproval {
        #[serde(default = "default_approval_method")]
        method: String,
        #[serde(default)]
        params: Value,
        /// When true, the fake **suspends** the turn after emitting this approval
        /// and resumes only once the client answers it — so a scenario can gate a
        /// turn on a real decision, and the fake, on resuming, echoes the
        /// accept/decline it received as an assistant message (observable proof
        /// the decision round-tripped). Default false keeps the fire-and-forget
        /// behavior (emit and continue without waiting).
        #[serde(default)]
        blocking: bool,
    },
    Notification {
        method: String,
        #[serde(default)]
        params: Value,
    },
}

fn default_approval_method() -> String {
    DEFAULT_APPROVAL_METHOD.to_owned()
}

/// What a `turn/start` request plays.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Turn {
    /// The id returned from `turn/start` (under `result.turn.id`) and wrapped
    /// into every emitted `turn/started` / `turn/completed` `params.turn`.
    #[serde(default = "default_turn_id")]
    pub turn_id: String,
    /// The notifications (and optional approval request) to emit, in order.
    #[serde(default)]
    pub emit: Vec<Emit>,
}

fn default_turn_id() -> String {
    "turn_fake_0001".to_owned()
}

/// A parsed scenario file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Scenario {
    /// Returned as `initialize` → `result.serverInfo`.
    #[serde(default = "default_server_info")]
    pub server_info: Value,
    /// The id returned from `thread/start` and stamped into notifications.
    #[serde(default = "default_thread_id")]
    pub thread_id: String,
    /// What a `turn/start` plays (nothing beyond a response when absent).
    #[serde(default)]
    pub turn: Option<Turn>,
}

fn default_server_info() -> Value {
    json!({ "name": "fake-codex", "version": "0" })
}

fn default_thread_id() -> String {
    DEFAULT_THREAD_ID.to_owned()
}

impl Scenario {
    /// Resolve the scenario for this launch: the explicit `FAKE_CODEX_SCENARIO`
    /// path, or the built-in default.
    pub fn resolve() -> Result<Self, String> {
        match std::env::var("FAKE_CODEX_SCENARIO") {
            Ok(path) => Self::load(&path),
            Err(_) => Ok(Self::default_scenario()),
        }
    }

    /// Load and parse a scenario file.
    pub fn load(path: &str) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("read scenario {path}: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("parse scenario {path}: {e}"))
    }

    /// The built-in default: one thread, one short assistant-message turn.
    pub fn default_scenario() -> Self {
        Self {
            server_info: default_server_info(),
            thread_id: DEFAULT_THREAD_ID.to_owned(),
            turn: Some(Turn {
                turn_id: default_turn_id(),
                emit: vec![
                    Emit::TurnStarted,
                    Emit::ItemStarted {
                        item: json!({ "id": "item_1", "itemType": "agent_message" }),
                    },
                    Emit::ItemCompleted {
                        item: json!({
                            "id": "item_1",
                            "itemType": "agent_message",
                            "text": "fake-codex scripted reply"
                        }),
                    },
                    Emit::TurnCompleted {
                        status: "completed".to_owned(),
                    },
                ],
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_full_vocabulary() {
        let scenario: Scenario = serde_json::from_str(
            r#"{
                "server_info": { "name": "s", "version": "1" },
                "thread_id": "thr_x",
                "turn": {
                    "turn_id": "turn_x",
                    "emit": [
                        { "type": "turn_started" },
                        { "type": "item_started", "item": { "id": "i1" } },
                        { "type": "item_completed", "item": { "id": "i1", "text": "hi" } },
                        { "type": "request_approval", "method": "turn/requestApproval", "params": { "k": 1 } },
                        { "type": "request_approval" },
                        { "type": "notification", "method": "server/note", "params": { "n": 2 } },
                        { "type": "turn_completed", "status": "completed" }
                    ]
                }
            }"#,
        )
        .unwrap();
        assert_eq!(scenario.thread_id, "thr_x");
        let turn = scenario.turn.unwrap();
        assert_eq!(turn.turn_id, "turn_x");
        assert_eq!(turn.emit.len(), 7);
        assert_eq!(turn.emit[0], Emit::TurnStarted);
        assert_eq!(
            turn.emit[4],
            Emit::RequestApproval {
                method: DEFAULT_APPROVAL_METHOD.to_owned(),
                params: Value::Null,
                blocking: false,
            }
        );
        assert_eq!(
            turn.emit[6],
            Emit::TurnCompleted {
                status: "completed".to_owned()
            }
        );
    }

    #[test]
    fn defaults_fill_in_for_a_minimal_scenario() {
        let scenario: Scenario = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(scenario.thread_id, DEFAULT_THREAD_ID);
        assert_eq!(scenario.server_info["name"], "fake-codex");
        assert!(scenario.turn.is_none());
    }

    #[test]
    fn the_built_in_default_has_a_playable_turn() {
        let scenario = Scenario::default_scenario();
        let turn = scenario.turn.expect("default has a turn");
        assert!(matches!(turn.emit.last(), Some(Emit::TurnCompleted { .. })));
    }
}
