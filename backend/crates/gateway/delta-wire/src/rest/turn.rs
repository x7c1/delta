//! A session's current turn state.

use delta_usecase::TurnState;
use serde::Serialize;
use ts_rs::TS;

use crate::rest::turn_phase::WireTurnPhase;

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
    /// The thread the in-flight turn is running on, so a reconnecting client can
    /// re-seed its per-thread running indicator on the exact thread. `null`
    /// while idle.
    pub thread_id: Option<i64>,
}

impl WireTurn {
    pub(super) fn from_state(state: TurnState, in_progress_thread: Option<i64>) -> Self {
        match state {
            TurnState::Idle => WireTurn {
                state: WireTurnPhase::Idle,
                send_id: None,
                thread_id: None,
            },
            TurnState::AwaitingEcho { send_id, .. } => WireTurn {
                state: WireTurnPhase::AwaitingEcho,
                send_id: Some(send_id),
                thread_id: in_progress_thread,
            },
            TurnState::InFlight { send_id } => WireTurn {
                state: WireTurnPhase::InFlight,
                send_id,
                thread_id: in_progress_thread,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_state_serializes_with_snake_case_phase_and_optional_send_id() {
        assert_eq!(
            serde_json::to_value(WireTurn::from_state(TurnState::Idle, Some(3))).unwrap(),
            serde_json::json!({ "state": "idle", "send_id": null, "thread_id": null }),
        );
        assert_eq!(
            serde_json::to_value(WireTurn::from_state(
                TurnState::AwaitingEcho {
                    send_id: 7,
                    slash_command: false,
                },
                Some(3),
            ))
            .unwrap(),
            serde_json::json!({ "state": "awaiting_echo", "send_id": 7, "thread_id": 3 }),
        );
        assert_eq!(
            serde_json::to_value(WireTurn::from_state(
                TurnState::InFlight { send_id: Some(7) },
                Some(3),
            ))
            .unwrap(),
            serde_json::json!({ "state": "in_flight", "send_id": 7, "thread_id": 3 }),
        );
        assert_eq!(
            serde_json::to_value(WireTurn::from_state(
                TurnState::InFlight { send_id: None },
                None,
            ))
            .unwrap(),
            serde_json::json!({ "state": "in_flight", "send_id": null, "thread_id": null }),
        );
    }
}
