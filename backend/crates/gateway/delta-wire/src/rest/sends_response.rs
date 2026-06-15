//! Response for `GET /api/sessions/{id}/sends`.

use delta_model::Send;
use delta_usecase::{
    PendingPermission, PendingQuestion, RunningSubagent, SessionLiveState, TurnState,
};
use serde::Serialize;
use ts_rs::TS;

use crate::send::WireSend;

/// The phase of a session's turn state machine, as reported on the REST
/// surface. Mirrors the domain `TurnState` variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "TurnPhase")]
pub enum WireTurnPhase {
    Idle,
    AwaitingEcho,
    InFlight,
}

/// A session's current turn state: the phase plus, when a Delta-dispatched
/// send drives the turn, its send id. `send_id` is `null` while idle and for a
/// turn started by external pane input.
///
/// This is queryable runtime state (not an event), so a client reconnecting
/// after a missed event window can rebuild its in-progress indicator from a
/// plain refetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "Turn")]
pub struct WireTurn {
    pub state: WireTurnPhase,
    pub send_id: Option<i64>,
}

impl From<TurnState> for WireTurn {
    fn from(state: TurnState) -> Self {
        match state {
            TurnState::Idle => WireTurn {
                state: WireTurnPhase::Idle,
                send_id: None,
            },
            TurnState::AwaitingEcho { send_id } => WireTurn {
                state: WireTurnPhase::AwaitingEcho,
                send_id: Some(send_id),
            },
            TurnState::InFlight { send_id } => WireTurn {
                state: WireTurnPhase::InFlight,
                send_id,
            },
        }
    }
}

/// A permission dialog currently awaiting a human answer, as reported on the
/// REST surface.
///
/// This is the queryable counterpart of the `permission_requested` event
/// (same fields, minus the session id the URL already names): the event is
/// lost for a client whose socket was down when it fired, so a reconnecting
/// client rebuilds its permission notice from this instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PendingPermission")]
pub struct WirePendingPermission {
    /// The request row id `POST /api/permissions/{id}/decision` answers.
    pub request_id: i64,
    pub tool_name: String,
    /// The tool input, serialized as JSON text.
    pub tool_input: String,
}

impl From<PendingPermission> for WirePendingPermission {
    fn from(pending: PendingPermission) -> Self {
        WirePendingPermission {
            request_id: pending.request_id,
            tool_name: pending.tool_name,
            tool_input: pending.tool_input_json,
        }
    }
}

/// An `AskUserQuestion` tool call currently presenting its options in the TUI,
/// as reported on the REST surface.
///
/// The queryable counterpart of the `question_asked` event (same fields, minus
/// the session id the URL already names): the event is lost for a client whose
/// socket was down when it fired, so a reconnecting client rebuilds its
/// question card from this instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PendingQuestion")]
pub struct WirePendingQuestion {
    /// The `PreToolUse` row id whose `tool_result` resolves this question.
    pub request_id: i64,
    /// The in-flight turn's thread, so a reconnecting client shows the question
    /// card only on the thread it belongs to.
    pub thread_id: i64,
    /// The raw `{"questions":[…]}` tool input, serialized as JSON text.
    pub tool_input: String,
}

impl From<PendingQuestion> for WirePendingQuestion {
    fn from(pending: PendingQuestion) -> Self {
        WirePendingQuestion {
            request_id: pending.request_id,
            thread_id: pending.thread_id.0,
            tool_input: pending.tool_input_json,
        }
    }
}

/// A subagent (the `Agent`/`Task` tool) currently running inside the session's
/// turn, as reported on the REST surface.
///
/// The queryable counterpart of the `subagent_started` event (minus the session
/// id the URL already names): the start/finish events are lost for a client
/// whose socket was down when they fired, so a reconnecting client rebuilds its
/// running-subagent indicator from this list instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RunningSubagent")]
pub struct WireRunningSubagent {
    /// The `tool_use_id` of the `Agent`/`Task` call (its stable key).
    pub tool_use_id: String,
    /// The subagent type (e.g. `general-purpose`), if the call carried one.
    pub subagent_type: Option<String>,
    /// The short task description, if the call carried one, for display.
    pub description: Option<String>,
    /// Whether the launch carried `run_in_background: true`. A reconnecting
    /// client carries this so its turn-end sweep keeps a surviving background
    /// subagent while dropping foreground ones.
    pub background: bool,
}

impl From<RunningSubagent> for WireRunningSubagent {
    fn from(subagent: RunningSubagent) -> Self {
        WireRunningSubagent {
            tool_use_id: subagent.tool_use_id,
            subagent_type: subagent.subagent_type,
            description: subagent.description,
            background: subagent.background,
        }
    }
}

/// Response for `GET /api/sessions/{id}/sends`: the session's open
/// (non-terminal) sends — status `queued` or `dispatched` — oldest first, plus
/// the session's queryable live state (the current turn state, the pending
/// permission dialog, the pending question, and the running subagents — each
/// present/non-empty only while something is in flight).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SendsResponse")]
pub struct WireSendsResponse {
    pub sends: Vec<WireSend>,
    pub turn: WireTurn,
    pub permission: Option<WirePendingPermission>,
    pub question: Option<WirePendingQuestion>,
    /// The subagents currently running in this session's turn, oldest first.
    /// Empty when none is running.
    pub running_subagents: Vec<WireRunningSubagent>,
}

impl WireSendsResponse {
    pub fn new(sends: Vec<Send>, live: SessionLiveState) -> Self {
        WireSendsResponse {
            sends: sends.into_iter().map(WireSend::from).collect(),
            turn: live.turn.into(),
            permission: live.pending_permission.map(WirePendingPermission::from),
            question: live.pending_question.map(WirePendingQuestion::from),
            running_subagents: live
                .running_subagents
                .into_iter()
                .map(WireRunningSubagent::from)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::ThreadId;

    #[test]
    fn turn_state_serializes_with_snake_case_phase_and_optional_send_id() {
        assert_eq!(
            serde_json::to_value(WireTurn::from(TurnState::Idle)).unwrap(),
            serde_json::json!({ "state": "idle", "send_id": null }),
        );
        assert_eq!(
            serde_json::to_value(WireTurn::from(TurnState::AwaitingEcho { send_id: 7 })).unwrap(),
            serde_json::json!({ "state": "awaiting_echo", "send_id": 7 }),
        );
        assert_eq!(
            serde_json::to_value(WireTurn::from(TurnState::InFlight { send_id: Some(7) })).unwrap(),
            serde_json::json!({ "state": "in_flight", "send_id": 7 }),
        );
        assert_eq!(
            serde_json::to_value(WireTurn::from(TurnState::InFlight { send_id: None })).unwrap(),
            serde_json::json!({ "state": "in_flight", "send_id": null }),
        );
    }

    #[test]
    fn sends_response_carries_sends_turn_permission_and_question() {
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::Idle,
                pending_permission: None,
                pending_question: None,
                running_subagents: Vec::new(),
            },
        );
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({
                "sends": [],
                "turn": { "state": "idle", "send_id": null },
                "permission": null,
                "question": null,
                "running_subagents": [],
            }),
        );
    }

    #[test]
    fn sends_response_reports_the_pending_permission_dialog() {
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::InFlight { send_id: Some(7) },
                pending_permission: Some(PendingPermission {
                    request_id: 3,
                    tool_name: "Bash".to_owned(),
                    tool_input_json: "{\"command\":\"rm -rf scratch\"}".to_owned(),
                }),
                pending_question: None,
                running_subagents: Vec::new(),
            },
        );
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({
                "sends": [],
                "turn": { "state": "in_flight", "send_id": 7 },
                "permission": {
                    "request_id": 3,
                    "tool_name": "Bash",
                    "tool_input": "{\"command\":\"rm -rf scratch\"}",
                },
                "question": null,
                "running_subagents": [],
            }),
        );
    }

    #[test]
    fn sends_response_reports_the_pending_question() {
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::InFlight { send_id: Some(7) },
                pending_permission: None,
                pending_question: Some(PendingQuestion {
                    request_id: 5,
                    thread_id: ThreadId(3),
                    tool_input_json: "{\"questions\":[{\"header\":\"Pick\"}]}".to_owned(),
                }),
                running_subagents: Vec::new(),
            },
        );
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({
                "sends": [],
                "turn": { "state": "in_flight", "send_id": 7 },
                "permission": null,
                "question": {
                    "request_id": 5,
                    "thread_id": 3,
                    "tool_input": "{\"questions\":[{\"header\":\"Pick\"}]}",
                },
                "running_subagents": [],
            }),
        );
    }

    #[test]
    fn sends_response_reports_the_running_subagents_oldest_first() {
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::InFlight { send_id: Some(7) },
                pending_permission: None,
                pending_question: None,
                running_subagents: vec![
                    RunningSubagent {
                        tool_use_id: "toolu_01".to_owned(),
                        subagent_type: Some("general-purpose".to_owned()),
                        description: Some("Probe the codebase".to_owned()),
                        background: false,
                    },
                    RunningSubagent {
                        tool_use_id: "toolu_02".to_owned(),
                        subagent_type: None,
                        description: None,
                        background: true,
                    },
                ],
            },
        );
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({
                "sends": [],
                "turn": { "state": "in_flight", "send_id": 7 },
                "permission": null,
                "question": null,
                "running_subagents": [
                    {
                        "tool_use_id": "toolu_01",
                        "subagent_type": "general-purpose",
                        "description": "Probe the codebase",
                        "background": false,
                    },
                    {
                        "tool_use_id": "toolu_02",
                        "subagent_type": null,
                        "description": null,
                        "background": true,
                    },
                ],
            }),
        );
    }
}
