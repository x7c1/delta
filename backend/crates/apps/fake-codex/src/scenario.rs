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
//!       { "type": "item_started",   "item": { "id": "item_1", "type": "agentMessage" } },
//!       { "type": "agent_message_delta", "item_id": "item_1", "delta": "hi" },
//!       { "type": "item_completed", "item": { "id": "item_1", "type": "agentMessage", "text": "hi" } },
//!       { "type": "request_approval", "method": "item/commandExecution/requestApproval", "params": { "itemId": "exec_1", "command": "date" } },
//!       { "type": "turn_completed", "status": "completed" }
//!     ]
//!   }
//! }
//! ```
//!
//! The `item` payloads are the **real** v2 `ThreadItem` shapes (discriminated by
//! `type`, e.g. `agentMessage` / `commandExecution`), and the item envelope /
//! streaming-delta steps use the real notification method names.
//!
//! Top-level fields:
//!
//! - `server_info` (optional): the object returned as `initialize` →
//!   `result.serverInfo`.
//! - `thread_id` (default `"thr_fake_0001"`): the id returned from
//!   `thread/start` and stamped into every emitted notification's
//!   `params.threadId` (unless the `turn/start` request itself named a thread).
//! - `model` (default `"fake-codex-model"`): the model reported as the
//!   `thread/start` / `thread/resume` response's top-level `model` — the model
//!   the server *resolved* for the thread, whatever the client asked for.
//! - `thread_start_delay_ms` (default `0`): how long the fake stalls before
//!   answering `thread/start`, standing in for a slow cold start of the real
//!   `codex app-server`. It widens the window between "the send was accepted"
//!   and "the session is live" enough for a test to observe the starting
//!   session, exactly as `fake-claude`'s `session_start.delay_ms` does on the
//!   pane path.
//! - `turn` (optional): what a `turn/start` request plays. When absent, a
//!   `turn/start` still gets a response but emits nothing.
//!
//! A `turn`'s `emit` list plays strictly in order once the `turn/start`
//! response is written. The step vocabulary:
//!
//! | step | effect |
//! |---|---|
//! | `item_started { item }` | Emit an `item/started` notification carrying `item` (plus the real `turnId` / `startedAtMs` envelope). |
//! | `item_completed { item }` | Emit an `item/completed` notification carrying `item` (plus the real `turnId` / `completedAtMs` envelope). |
//! | `agent_message_delta { item_id, delta }` | Emit an `item/agentMessage/delta` notification (`{ itemId, delta, turnId }`) — a streaming fragment of an assistant message. |
//! | `turn_started` | Emit a `turn/started` notification. |
//! | `turn_completed { status }` | Emit a `turn/completed` notification carrying `status` (e.g. `completed`, `interrupted`, `failed`). |
//! | `request_approval { method?, params?, blocking? }` | Emit a server → client request (default method `item/commandExecution/requestApproval`, the real command-execution approval) with a freshly minted id. With `blocking: false` (default) the fake emits and continues. With `blocking: true` the fake **suspends** the turn after emitting it and resumes only once the client answers; on resuming it echoes the decision string it received — `accept`, `acceptForSession` or `decline`, verbatim and unvalidated — as an assistant message before playing the rest of the turn. |
//! | `await_approvals` | **Suspend** the turn until *every* approval emitted so far has been answered. Each answer is echoed as an assistant message the moment it arrives; the parked remainder plays once the last outstanding approval is answered. A no-op when nothing is outstanding. |
//! | `exit` | **Die.** The fake exits immediately, without emitting anything further — so the client's reader sees its stdout close, exactly as when a real `codex app-server` process is killed or crashes mid-turn. Everything scripted after this step never plays. |
//! | `notification { method, params? }` | Emit an arbitrary notification (escape hatch for shapes not covered above). |
//! | `account_notification { method, params? }` | Emit an **account-scoped** notification: the params are written verbatim, WITHOUT a `threadId`, exactly as the real server emits `account/rateLimits/updated`. |
//!
//! Every emitted notification (and the approval request) gets `threadId` stamped
//! into its `params` so the client transport can demux it to the right thread —
//! *except* `account_notification`, and that exception is the point. A real
//! `codex app-server` emits account-scoped frames (the rate-limit rolling update)
//! with no thread at all, so they take the client's connection-level unrouted
//! path rather than its per-thread demux. A fake that stamped a `threadId` onto
//! them would exercise the routed path instead, and a test written against it
//! would pass while the real path was broken.
//!
//! Several non-blocking `request_approval` steps followed by `await_approvals`
//! reproduce what a real `codex app-server` does on a parallel tool-call
//! fan-out: N approval requests outstanding at once, the turn finishing only
//! once every one of them has an answer. `blocking: true` is the serial case —
//! one approval gating the turn — and cannot express the parallel one, since it
//! stops the script at the first request.
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

/// The default method for a scripted server → client approval request: the real
/// command-execution approval a `turn/start` turn drives (see the vendored
/// `ServerRequest` schema). A scenario overrides it (e.g. with
/// `item/fileChange/requestApproval`) via the step's `method` field.
pub const DEFAULT_APPROVAL_METHOD: &str = "item/commandExecution/requestApproval";
/// The default thread id when a scenario does not name one.
pub const DEFAULT_THREAD_ID: &str = "thr_fake_0001";
/// The default resolved model when a scenario does not name one. Deliberately
/// unlike any real model name, so a value that leaks into an assertion is
/// obviously the fake's.
pub const DEFAULT_MODEL: &str = "fake-codex-model";

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
    /// A streaming fragment of an assistant message: emits the real
    /// `item/agentMessage/delta` notification (`{ itemId, delta, turnId }`).
    AgentMessageDelta {
        item_id: String,
        delta: String,
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
        /// decision string it received as an assistant message (observable proof
        /// the decision round-tripped, and the only way a test can see *which*
        /// value went over the wire). Default false keeps the fire-and-forget
        /// behavior (emit and continue without waiting).
        #[serde(default)]
        blocking: bool,
    },
    /// Suspend the turn until **every** approval emitted so far has been
    /// answered, then play the rest of the turn.
    ///
    /// This is the parallel-approval gate: a scenario emits N non-blocking
    /// `request_approval` steps (so all N are outstanding at once, as a real
    /// `codex app-server` does when the model fans tool calls out) and then parks
    /// the turn here. Each answer is echoed as an assistant message as it
    /// arrives, so a test can prove every request round-tripped — and the turn
    /// completes only after the last one, which is what makes an unanswerable
    /// request show up as a hung turn rather than a silent pass.
    ///
    /// A no-op when no approval is outstanding.
    AwaitApprovals,
    /// Die where a real app-server would: the fake exits at this point in the
    /// turn, closing its stdout, so the client sees the EOF a killed or crashed
    /// `codex app-server` produces.
    ///
    /// This is how a scenario re-enacts the field failure — a process that goes
    /// away mid-turn, with approvals outstanding and a turn still in flight —
    /// deterministically, without a test having to find and signal a PID. Nothing
    /// scripted after it ever plays.
    Exit,
    Notification {
        method: String,
        #[serde(default)]
        params: Value,
    },
    /// An account-scoped notification, emitted with its params **verbatim** —
    /// no `threadId` is stamped in. See the module docs for why the distinction
    /// from [`Emit::Notification`] is load-bearing.
    AccountNotification {
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
    /// How long to stall before answering `thread/start`, in milliseconds.
    ///
    /// A real `codex app-server` takes a moment to come up and start a thread,
    /// and a session is only live once it has. Delta accepts the new-session
    /// send before any of that runs, so the honest way to test what the browser
    /// shows in between is to make that window wide enough to look at. `0` (the
    /// default) keeps every other scenario prompt, which matters: the shared
    /// Codex specs wait for output at the default timeout.
    #[serde(default)]
    pub thread_start_delay_ms: u64,
    /// The model reported as the top-level `model` of the `thread/start` /
    /// `thread/resume` response — what the server *resolved* for the thread,
    /// which a real `codex app-server` decides from the request, the user's
    /// config and its own default. A scenario names a distinctive value so a
    /// test can prove the client reports the server's answer rather than
    /// echoing what it asked for.
    #[serde(default = "default_model")]
    pub model: String,
    /// What a `turn/start` plays (nothing beyond a response when absent). Used
    /// when [`Self::turns`] is empty; the same turn is replayed on every
    /// `turn/start` (its ids are therefore reused across turns).
    #[serde(default)]
    pub turn: Option<Turn>,
    /// A sequence of turns played one per `turn/start`, in order, when non-empty
    /// — so successive turns of one session can carry DISTINCT turn/item ids,
    /// mirroring a real `codex app-server` (which mints a fresh turn per prompt).
    /// The last entry is replayed once the sequence is exhausted. When empty the
    /// fake falls back to the single [`Self::turn`], so every existing
    /// single-turn scenario is unchanged.
    #[serde(default)]
    pub turns: Vec<Turn>,
}

fn default_server_info() -> Value {
    json!({ "name": "fake-codex", "version": "0" })
}

fn default_thread_id() -> String {
    DEFAULT_THREAD_ID.to_owned()
}

fn default_model() -> String {
    DEFAULT_MODEL.to_owned()
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

    /// The turn to play for the `turn/start` at zero-based `index` within one
    /// session's process.
    ///
    /// When [`Self::turns`] is provided, turns play in order and the last entry
    /// is replayed once exhausted; otherwise the single [`Self::turn`] is used
    /// (and thus replayed on every turn). Cloned so the caller can play it
    /// without holding a borrow of the scenario.
    pub fn turn_at(&self, index: usize) -> Option<Turn> {
        if !self.turns.is_empty() {
            return self.turns.get(index).or_else(|| self.turns.last()).cloned();
        }
        self.turn.clone()
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
            thread_start_delay_ms: 0,
            model: DEFAULT_MODEL.to_owned(),
            turn: Some(Turn {
                turn_id: default_turn_id(),
                emit: vec![
                    Emit::TurnStarted,
                    Emit::ItemStarted {
                        item: json!({ "id": "item_1", "type": "agentMessage" }),
                    },
                    Emit::AgentMessageDelta {
                        item_id: "item_1".to_owned(),
                        delta: "fake-codex scripted reply".to_owned(),
                    },
                    Emit::ItemCompleted {
                        item: json!({
                            "id": "item_1",
                            "type": "agentMessage",
                            "text": "fake-codex scripted reply"
                        }),
                    },
                    Emit::TurnCompleted {
                        status: "completed".to_owned(),
                    },
                ],
            }),
            turns: Vec::new(),
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
                        { "type": "item_started", "item": { "id": "i1", "type": "agentMessage" } },
                        { "type": "agent_message_delta", "item_id": "i1", "delta": "hi" },
                        { "type": "item_completed", "item": { "id": "i1", "type": "agentMessage", "text": "hi" } },
                        { "type": "request_approval", "method": "item/fileChange/requestApproval", "params": { "itemId": "fc_1" } },
                        { "type": "request_approval" },
                        { "type": "notification", "method": "server/note", "params": { "n": 2 } },
                        { "type": "account_notification", "method": "account/rateLimits/updated", "params": { "rateLimits": { "primary": { "usedPercent": 7 } } } },
                        { "type": "turn_completed", "status": "completed" }
                    ]
                }
            }"#,
        )
        .unwrap();
        assert_eq!(scenario.thread_id, "thr_x");
        let turn = scenario.turn.unwrap();
        assert_eq!(turn.turn_id, "turn_x");
        assert_eq!(turn.emit.len(), 9);
        assert_eq!(turn.emit[0], Emit::TurnStarted);
        assert_eq!(
            turn.emit[2],
            Emit::AgentMessageDelta {
                item_id: "i1".to_owned(),
                delta: "hi".to_owned(),
            }
        );
        assert_eq!(
            turn.emit[5],
            Emit::RequestApproval {
                method: DEFAULT_APPROVAL_METHOD.to_owned(),
                params: Value::Null,
                blocking: false,
            }
        );
        assert_eq!(
            turn.emit[7],
            Emit::AccountNotification {
                method: "account/rateLimits/updated".to_owned(),
                params: json!({ "rateLimits": { "primary": { "usedPercent": 7 } } }),
            }
        );
        assert_eq!(
            turn.emit[8],
            Emit::TurnCompleted {
                status: "completed".to_owned()
            }
        );
    }

    #[test]
    fn parses_a_parallel_approval_fan_out() {
        // The parallel shape: several non-blocking approvals (all outstanding at
        // once) parked on `await_approvals`, with the turn end behind the gate.
        let scenario: Scenario = serde_json::from_str(
            r#"{
                "turn": {
                    "emit": [
                        { "type": "request_approval", "params": { "itemId": "e1", "command": "cat a" } },
                        { "type": "request_approval", "params": { "itemId": "e2", "command": "cat b" } },
                        { "type": "await_approvals" },
                        { "type": "turn_completed", "status": "completed" }
                    ]
                }
            }"#,
        )
        .unwrap();
        let turn = scenario.turn.expect("a turn");
        assert!(
            turn.emit[..2].iter().all(|e| matches!(
                e,
                Emit::RequestApproval {
                    blocking: false,
                    ..
                }
            )),
            "the fan-out does not block between requests: {:?}",
            turn.emit
        );
        assert_eq!(turn.emit[2], Emit::AwaitApprovals);
    }

    #[test]
    fn parses_a_mid_turn_death() {
        // The death shape: an approval goes out and the process dies with it
        // still unanswered — the field failure a settle test drives.
        let scenario: Scenario = serde_json::from_str(
            r#"{
                "turn": {
                    "emit": [
                        { "type": "request_approval", "params": { "itemId": "e1", "command": "cat a" } },
                        { "type": "exit" },
                        { "type": "turn_completed", "status": "completed" }
                    ]
                }
            }"#,
        )
        .unwrap();
        let turn = scenario.turn.expect("a turn");
        assert_eq!(turn.emit[1], Emit::Exit);
        assert_eq!(
            turn.emit.len(),
            3,
            "the steps after the death are parsed (and never played)"
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
