//! Response for `GET /api/sessions/{id}/sends`.

use delta_model::Send;
use delta_usecase::TurnState;
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

/// Response for `GET /api/sessions/{id}/sends`: the session's open
/// (non-terminal) sends — status `queued` or `dispatched` — oldest first, plus
/// the session's current turn state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SendsResponse")]
pub struct WireSendsResponse {
    pub sends: Vec<WireSend>,
    pub turn: WireTurn,
}

impl WireSendsResponse {
    pub fn new(sends: Vec<Send>, turn: TurnState) -> Self {
        WireSendsResponse {
            sends: sends.into_iter().map(WireSend::from).collect(),
            turn: turn.into(),
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
    fn sends_response_carries_sends_and_turn() {
        let body = WireSendsResponse::new(Vec::new(), TurnState::Idle);
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({
                "sends": [],
                "turn": { "state": "idle", "send_id": null },
            }),
        );
    }
}
