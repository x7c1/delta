//! Response for `GET /api/sessions/{id}/sends`.

use delta_model::Send;
use delta_usecase::{
    PendingPermission, PendingQuestion, RunningSubagent, SessionLiveState, TurnState,
};
use serde::Serialize;
use ts_rs::TS;

use crate::file_change::WireFileChangeDetail;
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
    /// The thread the in-flight turn is running on, so a reconnecting client can
    /// re-seed its per-thread running indicator on the exact thread. `null`
    /// while idle.
    pub thread_id: Option<i64>,
}

impl WireTurn {
    fn from_state(state: TurnState, in_progress_thread: Option<i64>) -> Self {
        match state {
            TurnState::Idle => WireTurn {
                state: WireTurnPhase::Idle,
                send_id: None,
                thread_id: None,
            },
            TurnState::AwaitingEcho { send_id } => WireTurn {
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

/// A permission dialog currently awaiting a human answer, as reported on the
/// REST surface.
///
/// This is the queryable counterpart of the `permission_requested` event
/// (same fields, minus the session id the URL already names): the event is
/// lost for a client whose socket was down when it fired, so a reconnecting
/// client rebuilds its permission notice from this instead.
///
/// Several requests can be pending at once (a provider running tool calls in
/// parallel), and the envelope reports the queue's **head** here plus the depth
/// in `permission_count` — the dialog to show, and how many answers are owed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PendingPermission")]
pub struct WirePendingPermission {
    /// The request row id `POST /api/permissions/{id}/decision` answers.
    pub request_id: i64,
    pub tool_name: String,
    /// The tool input, serialized as JSON text.
    pub tool_input: String,
    /// What allowing the request would do to files on disk, when the provider
    /// stated it. The same detail the `permission_requested` event carries, so a
    /// client that missed that event rebuilds the *same* card from this refetch
    /// rather than one degraded to the input summary.
    ///
    /// Absent from the envelope entirely when nothing is known, which is every
    /// request that is not a file change.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub file_change: Option<WireFileChangeDetail>,
    /// A directory the request also asks to be allowed to write under for the
    /// rest of the session, when the provider asked for one — the same field the
    /// `permission_requested` event carries, and independent of `file_change` in
    /// the same way: a request can carry this and no change set at all.
    ///
    /// Absent from the envelope entirely when the provider asked for no root.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub grant_root: Option<String>,
}

impl From<&PendingPermission> for WirePendingPermission {
    fn from(pending: &PendingPermission) -> Self {
        WirePendingPermission {
            request_id: pending.request_id,
            tool_name: pending.tool_name.clone(),
            tool_input: pending.tool_input_json.clone(),
            file_change: pending.file_change.as_ref().map(WireFileChangeDetail::from),
            grant_root: pending.grant_root.clone(),
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

/// A subagent currently running for the session, as reported on the REST
/// surface: an `Agent`/`Task` tool call inside a turn, or the background agent
/// Claude Code forks for a slash command's skill, which runs with no turn in
/// flight at all.
///
/// The queryable counterpart of the `subagent_started` event (minus the session
/// id the URL already names): the start/finish events are lost for a client
/// whose socket was down when they fired, so a reconnecting client rebuilds its
/// running-subagent indicator from this list instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RunningSubagent")]
pub struct WireRunningSubagent {
    /// The thread that launched the subagent, so a reconnecting client can keep
    /// that thread's running indicator lit — and its unread badge suppressed —
    /// until the subagent finishes, which for a background subagent outlives the
    /// launching turn.
    pub thread_id: i64,
    /// The launch's `tool_use_id`, its stable key — synthetic
    /// (`forked-skill:<agentId>`) for a forked skill, which makes no tool call.
    pub tool_use_id: String,
    /// The subagent type (e.g. `general-purpose`), a forked skill's skill name,
    /// or null if the launch carried none.
    pub subagent_type: Option<String>,
    /// The short task description, if the launch carried one, for display.
    pub description: Option<String>,
    /// Whether the launch runs in the background — `run_in_background: true` for
    /// a tool call, and always true for a forked skill. A reconnecting client
    /// carries this so its turn-end sweep keeps a surviving background subagent
    /// while dropping foreground ones.
    pub background: bool,
}

impl From<RunningSubagent> for WireRunningSubagent {
    fn from(subagent: RunningSubagent) -> Self {
        WireRunningSubagent {
            thread_id: subagent.thread_id.0,
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

    use delta_model::ThreadId;
    use delta_usecase::{AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind};

    #[test]
    fn turn_state_serializes_with_snake_case_phase_and_optional_send_id() {
        assert_eq!(
            serde_json::to_value(WireTurn::from_state(TurnState::Idle, Some(3))).unwrap(),
            serde_json::json!({ "state": "idle", "send_id": null, "thread_id": null }),
        );
        assert_eq!(
            serde_json::to_value(WireTurn::from_state(
                TurnState::AwaitingEcho { send_id: 7 },
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
