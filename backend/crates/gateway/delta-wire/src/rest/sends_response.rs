//! Response for `GET /api/sessions/{id}/sends`.

use delta_model::Send;
use delta_usecase::SessionLiveState;
use serde::Serialize;
use ts_rs::TS;

use crate::rest::pending_permission::WirePendingPermission;
use crate::rest::pending_question::WirePendingQuestion;
use crate::rest::running_subagent::WireRunningSubagent;
use crate::rest::turn::WireTurn;
use crate::send::WireSend;

/// Response for `GET /api/sessions/{id}/sends`: the session's open
/// (non-terminal) sends — status `queued` or `dispatched` — oldest first, plus
/// the session's queryable live state (the current turn state, the pending
/// permission queue, the pending question, and the running subagents — each
/// present/non-empty only while something is in flight).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SendsResponse")]
pub struct WireSendsResponse {
    pub sends: Vec<WireSend>,
    pub turn: WireTurn,
    /// The permission dialog to show: the head of the session's pending-approval
    /// queue, or `null` when nothing is pending.
    pub permission: Option<WirePendingPermission>,
    /// How many permission requests are pending in total, `permission` included
    /// (`0` when it is `null`). A parallel-tool-call provider can leave several
    /// outstanding at once, so a client that refetches after a reconnect can
    /// rebuild both the dialog *and* its "N approvals pending" indication without
    /// having seen a single event. The remaining requests surface one at a time:
    /// answering the head promotes the next.
    pub permission_count: usize,
    pub question: Option<WirePendingQuestion>,
    /// The subagents currently running in this session's turn, oldest first.
    /// Empty when none is running.
    pub running_subagents: Vec<WireRunningSubagent>,
}

impl WireSendsResponse {
    pub fn new(sends: Vec<Send>, live: SessionLiveState) -> Self {
        WireSendsResponse {
            sends: sends.into_iter().map(WireSend::from).collect(),
            turn: WireTurn::from_state(live.turn, live.in_progress_thread.map(|id| id.0)),
            permission: live.pending_permission().map(WirePendingPermission::from),
            permission_count: live.pending_permissions.len(),
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

    use delta_usecase::{PendingPermission, PendingQuestion, RunningSubagent, TurnState};

    use delta_model::ThreadId;
    use delta_usecase::{AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind};

    #[test]
    fn sends_response_carries_sends_turn_permission_and_question() {
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::Idle,
                in_progress_thread: None,
                pending_permissions: Vec::new(),
                pending_question: None,
                running_subagents: Vec::new(),
            },
        );
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({
                "sends": [],
                "turn": { "state": "idle", "send_id": null, "thread_id": null },
                "permission": null,
                "permission_count": 0,
                "question": null,
                "running_subagents": [],
            }),
        );
    }

    /// One pending dialog: reported as the head, with a depth of 1.
    ///
    /// It carries no file-change detail (a `Bash` call changes nothing up front,
    /// and the Claude path never states one), so the envelope is byte-for-byte
    /// the one it has always been — no `file_change` key at all.
    #[test]
    fn sends_response_reports_the_pending_permission_dialog() {
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::InFlight { send_id: Some(7) },
                in_progress_thread: Some(ThreadId(2)),
                pending_permissions: vec![PendingPermission {
                    request_id: 3,
                    tool_name: "Bash".to_owned(),
                    tool_input_json: "{\"command\":\"rm -rf scratch\"}".to_owned(),
                    file_change: None,
                    grant_root: None,
                }],
                pending_question: None,
                running_subagents: Vec::new(),
            },
        );
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({
                "sends": [],
                "turn": { "state": "in_flight", "send_id": 7, "thread_id": 2 },
                "permission": {
                    "request_id": 3,
                    "tool_name": "Bash",
                    "tool_input": "{\"command\":\"rm -rf scratch\"}",
                },
                "permission_count": 1,
                "question": null,
                "running_subagents": [],
            }),
        );
    }

    /// A file-change dialog: the envelope carries the same detail the
    /// `permission_requested` event does, so a client that missed the event
    /// rebuilds the same card from a refetch instead of one degraded to the
    /// input summary.
    #[test]
    fn sends_response_reports_a_pending_file_change_with_its_detail() {
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::InFlight { send_id: None },
                in_progress_thread: Some(ThreadId(2)),
                pending_permissions: vec![PendingPermission {
                    request_id: 4,
                    tool_name: "file_change".to_owned(),
                    tool_input_json: "{\"itemId\":\"fc_1\"}".to_owned(),
                    file_change: Some(AgentFileChangeDetail {
                        changes: vec![AgentFileChange {
                            path: "src/lib.rs".to_owned(),
                            kind: Some(AgentFileChangeKind::Update),
                            diff: "@@ -1 +1 @@".to_owned(),
                        }],
                        reason: None,
                    }),
                    grant_root: None,
                }],
                pending_question: None,
                running_subagents: Vec::new(),
            },
        );
        assert_eq!(
            serde_json::to_value(body).unwrap()["permission"],
            serde_json::json!({
                "request_id": 4,
                "tool_name": "file_change",
                "tool_input": "{\"itemId\":\"fc_1\"}",
                "file_change": {
                    "changes": [
                        { "path": "src/lib.rs", "kind": "update", "diff": "@@ -1 +1 @@" },
                    ],
                    "reason": null,
                },
            }),
        );
    }

    /// A dialog whose change set could not be correlated but which asks for a
    /// write root: the envelope reports the root on its own. The re-seed must
    /// not be the surface that loses it — a reconnecting user answering from a
    /// refetched card is agreeing to writes under a whole tree, and this is the
    /// only place that card learns so.
    #[test]
    fn sends_response_reports_a_grant_root_without_a_change_set() {
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::InFlight { send_id: None },
                in_progress_thread: Some(ThreadId(2)),
                pending_permissions: vec![PendingPermission {
                    request_id: 5,
                    tool_name: "file_change".to_owned(),
                    tool_input_json: "{\"itemId\":\"fc_2\"}".to_owned(),
                    file_change: None,
                    grant_root: Some("/repo".to_owned()),
                }],
                pending_question: None,
                running_subagents: Vec::new(),
            },
        );
        assert_eq!(
            serde_json::to_value(body).unwrap()["permission"],
            serde_json::json!({
                "request_id": 5,
                "tool_name": "file_change",
                "tool_input": "{\"itemId\":\"fc_2\"}",
                "grant_root": "/repo",
            }),
        );
    }

    /// Several pending dialogs (a parallel tool-call fan-out): the envelope
    /// reports the QUEUE HEAD plus the depth, so a client that missed every
    /// event rebuilds both the dialog and its "N approvals pending" indication
    /// from this one refetch.
    #[test]
    fn sends_response_reports_the_queue_head_and_the_pending_count() {
        let pending = |request_id: i64, command: &str| PendingPermission {
            request_id,
            tool_name: "exec_command".to_owned(),
            tool_input_json: format!("{{\"command\":\"{command}\"}}"),
            file_change: None,
            grant_root: None,
        };
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::InFlight { send_id: None },
                in_progress_thread: Some(ThreadId(2)),
                pending_permissions: vec![
                    pending(11, "cat a"),
                    pending(12, "cat b"),
                    pending(13, "cat c"),
                ],
                pending_question: None,
                running_subagents: Vec::new(),
            },
        );
        let value = serde_json::to_value(body).unwrap();
        assert_eq!(
            value["permission"],
            serde_json::json!({
                "request_id": 11,
                "tool_name": "exec_command",
                "tool_input": "{\"command\":\"cat a\"}",
            }),
            "the OLDEST request is the head — not the last writer"
        );
        assert_eq!(
            value["permission_count"],
            serde_json::json!(3),
            "the depth counts the head too"
        );
    }

    #[test]
    fn sends_response_reports_the_pending_question() {
        let body = WireSendsResponse::new(
            Vec::new(),
            SessionLiveState {
                turn: TurnState::InFlight { send_id: Some(7) },
                in_progress_thread: Some(ThreadId(3)),
                pending_permissions: Vec::new(),
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
                "turn": { "state": "in_flight", "send_id": 7, "thread_id": 3 },
                "permission": null,
                "permission_count": 0,
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
                in_progress_thread: Some(ThreadId(2)),
                pending_permissions: Vec::new(),
                pending_question: None,
                running_subagents: vec![
                    RunningSubagent {
                        thread_id: ThreadId(2),
                        tool_use_id: "toolu_01".to_owned(),
                        task_id: None,
                        subagent_type: Some("general-purpose".to_owned()),
                        description: Some("Probe the codebase".to_owned()),
                        background: false,
                    },
                    RunningSubagent {
                        thread_id: ThreadId(4),
                        tool_use_id: "toolu_02".to_owned(),
                        task_id: None,
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
                "turn": { "state": "in_flight", "send_id": 7, "thread_id": 2 },
                "permission": null,
                "permission_count": 0,
                "question": null,
                "running_subagents": [
                    {
                        "thread_id": 2,
                        "tool_use_id": "toolu_01",
                        "subagent_type": "general-purpose",
                        "description": "Probe the codebase",
                        "background": false,
                    },
                    {
                        "thread_id": 4,
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
