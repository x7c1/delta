//! Response for `GET /api/sessions/{id}/sends`.

use delta_model::Send;
use delta_usecase::{PendingPermission, PendingQuestion, SessionLiveState, TurnState};
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
    /// The raw `{"questions":[…]}` tool input, serialized as JSON text.
    pub tool_input: String,
}

impl From<PendingQuestion> for WirePendingQuestion {
    fn from(pending: PendingQuestion) -> Self {
        WirePendingQuestion {
            request_id: pending.request_id,
            tool_input: pending.tool_input_json,
        }
    }
}

/// Response for `GET /api/sessions/{id}/sends`: the session's open
/// (non-terminal) sends — status `queued` or `dispatched` — oldest first, plus
/// the session's queryable live state (the current turn state, the pending
/// permission dialog, and the pending question, each present only when one
/// awaits an answer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SendsResponse")]
pub struct WireSendsResponse {
    pub sends: Vec<WireSend>,
    pub turn: WireTurn,
    pub permission: Option<WirePendingPermission>,
    pub question: Option<WirePendingQuestion>,
}

impl WireSendsResponse {
    pub fn new(sends: Vec<Send>, live: SessionLiveState) -> Self {
        WireSendsResponse {
            sends: sends.into_iter().map(WireSend::from).collect(),
            turn: live.turn.into(),
            permission: live.pending_permission.map(WirePendingPermission::from),
            question: live.pending_question.map(WirePendingQuestion::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            serde_json::to_value(WireTurn::from(TurnState::InFlight { send_id: Some(7) }))
                .unwrap(),
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
            },
        );
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({
                "sends": [],
                "turn": { "state": "idle", "send_id": null },
                "permission": null,
                "question": null,
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
                    tool_input_json: "{\"questions\":[{\"header\":\"Pick\"}]}".to_owned(),
                }),
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
                    "tool_input": "{\"questions\":[{\"header\":\"Pick\"}]}",
                },
            }),
        );
    }
}
