//! One session in the list: the stored record plus its live state and trunk.

use delta_usecase::SessionListing;
use serde::Serialize;
use ts_rs::TS;

use crate::rest::running_subagent::WireRunningSubagent;
use crate::rest::turn::WireTurn;
use crate::session::WireSession;
use crate::thread::WireThread;

/// One session in the list: the stored record plus its live state and trunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SessionListItem")]
pub struct WireSessionListItem {
    pub session: WireSession,
    /// Whether the session currently has a live pane (resumable without
    /// `--resume`). A closed session still appears, with `open: false`.
    pub open: bool,
    /// Whether the session holds a pane the embedded terminal may attach to
    /// that nothing has bound yet — a launch whose pane is up while its first
    /// hook has not arrived. That is the window in which the launch may be
    /// waiting on an interactive prompt only a human can answer, so the row
    /// carries the fact and any browser (including one that reloaded mid-launch
    /// and never saw `spawn_pane_ready`) can offer the terminal. Mutually
    /// exclusive with `open`.
    pub pane_starting: bool,
    /// Whether the session is open on a pane Delta re-adopted after a restart
    /// but whose agent can no longer deliver hooks to this server, because the
    /// hook endpoint (port or secret) changed between the two runs. Its
    /// transcript and terminal still work; prompt echoes, turn ends and
    /// permission dialogs do not arrive. The browser shows a notice telling the
    /// user to use the terminal, or to close the session and send again (which
    /// resumes it with fresh settings). Always `false` for a session that is
    /// not open.
    pub hooks_unreachable: bool,
    pub main_thread_id: i64,
    /// Every thread of the session — the trunk and its branches — in the shape
    /// and order `GET /api/sessions/{id}/threads` returns them (ascending id).
    /// Carried on the row so a client can draw each session's thread tree from
    /// the list alone, without a request per session.
    pub threads: Vec<WireThread>,
    /// Timestamp of the session's most recent message (ISO-8601 UTC), or `null`
    /// when the session has no messages yet.
    pub last_activity_at: Option<String>,
    /// The session's current turn, in the shape and with the thread
    /// `GET /api/sessions/{id}/sends` reports it, so a client starting from
    /// nothing (a reload, or a reconnect that dropped its event-built flags)
    /// can restore every running session's indicator from the list, not only
    /// the one it has open. Idle for a session that is not live.
    pub turn: WireTurn,
    /// The subagents currently running in the session, oldest first — the
    /// sends envelope's `running_subagents`, carried for the same reason as
    /// `turn`. Empty for a session that is not live.
    pub running_subagents: Vec<WireRunningSubagent>,
}

impl From<SessionListing> for WireSessionListItem {
    fn from(listing: SessionListing) -> Self {
        WireSessionListItem {
            session: listing.session.into(),
            open: listing.open,
            pane_starting: listing.pane_starting,
            hooks_unreachable: listing.hooks_unreachable,
            main_thread_id: listing.main_thread_id.0,
            threads: listing.threads.into_iter().map(WireThread::from).collect(),
            last_activity_at: listing.last_activity_at,
            turn: WireTurn::from_state(listing.turn, listing.in_progress_thread.map(|id| id.0)),
            running_subagents: listing
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
    use delta_usecase::{RunningSubagent, TurnState};

    use crate::testing;

    /// A row with a turn in flight carries it in the sends envelope's `turn`
    /// shape, naming the thread it runs on, and its running subagents in the
    /// envelope's `running_subagents` shape.
    #[test]
    fn a_list_item_carries_its_turn_in_flight_and_running_subagents() {
        let mut row = testing::listing();
        row.turn = TurnState::InFlight { send_id: Some(7) };
        row.in_progress_thread = Some(ThreadId(2));
        row.running_subagents = vec![RunningSubagent {
            thread_id: ThreadId(2),
            tool_use_id: "toolu_01".to_owned(),
            task_id: None,
            subagent_type: Some("general-purpose".to_owned()),
            description: None,
            background: true,
        }];
        let value = serde_json::to_value(WireSessionListItem::from(row)).unwrap();
        assert_eq!(
            value["turn"],
            serde_json::json!({ "state": "in_flight", "send_id": 7, "thread_id": 2 }),
        );
        assert_eq!(
            value["running_subagents"],
            serde_json::json!([{
                "thread_id": 2,
                "tool_use_id": "toolu_01",
                "subagent_type": "general-purpose",
                "description": null,
                "background": true,
            }]),
        );
    }

    /// An idle row reports the idle turn with no thread, and no subagents.
    #[test]
    fn an_idle_list_item_carries_the_idle_turn() {
        let value = serde_json::to_value(WireSessionListItem::from(testing::listing())).unwrap();
        assert_eq!(
            value["turn"],
            serde_json::json!({ "state": "idle", "send_id": null, "thread_id": null }),
        );
        assert_eq!(value["running_subagents"], serde_json::json!([]));
    }
}
