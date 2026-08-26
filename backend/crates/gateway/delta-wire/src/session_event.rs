//! The wire form of [`SessionEvent`].

use delta_usecase::{RateLimitWindow, SessionEvent, StatusSnapshot};
use serde::Serialize;
use ts_rs::TS;

use crate::file_change::WireFileChangeDetail;
use crate::session::WireAgentProvider;

/// JSON shape of a session event on the `/ws` stream.
///
/// Mirrors the domain [`SessionEvent`] variant-for-variant; see that type for
/// the semantics of each event. This wire twin carries the serialization
/// concerns the domain type must not know about: the `kind` tag, the
/// snake_case variant names, and the TypeScript export. Ids are plain
/// `String`/`i64` here because that is exactly what crosses the wire.
///
/// Only `PartialEq` (not `Eq`) is derived: [`WireSessionEvent::StatusUpdated`]
/// carries a [`WireStatusSnapshot`] with `f64` fields, and `f64` does not
/// implement `Eq`. Nothing keys these events by hash, so `PartialEq` (which
/// backs `assert_eq!`) is all the equality they need.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(rename = "SessionEvent")]
pub enum WireSessionEvent {
    /// The session was registered (first `UserPromptSubmit`).
    SessionRegistered { session_id: String },
    /// A known, previously-closed session became live again (resumed).
    SessionOpened { session_id: String },
    /// An open session was closed: its pane was torn down but its data remains.
    SessionClosed { session_id: String },
    /// A held (`queued`) send was promoted to `dispatched` and typed.
    SendDispatched { session_id: String, send_id: i64 },
    /// A dispatched send was returned to the queue, held for an explicit
    /// release, after nothing was heard about it twice running: the row is
    /// `queued` with `held_at` set (so it stays in the open-send list with
    /// Send and Cancel actions, and never auto-dispatches) instead of being
    /// re-typed forever. The event is what explains *why* that row is waiting;
    /// `text` repeats the composed message so a client can name it without
    /// refetching the open-send list, which is where the message itself now
    /// lives.
    SendParked {
        session_id: String,
        send_id: i64,
        text: String,
    },
    /// A queued send was confirmed as a turn start. `thread_id` is the thread
    /// the dispatched send took its turn on, so the running indicator lights on
    /// the exact thread rather than the whole session.
    TurnStarted {
        session_id: String,
        send_id: i64,
        thread_id: i64,
        matched_uuid: String,
    },
    /// External input was detected (typed directly into the pane).
    ExternalInput { session_id: String, prompt: String },
    /// A response completed. `thread_id` is the thread whose in-flight turn just
    /// ended, so the running indicator clears (and an unread badge bumps when
    /// the thread is not focused) on the exact thread. `null` only for a `Stop`
    /// on a session that was never registered (no thread to resolve).
    TurnCompleted {
        session_id: String,
        thread_id: Option<i64>,
        stop_reason: Option<String>,
    },
    /// The in-flight turn was interrupted by the user (Escape / Ctrl-C).
    /// `thread_id` is the interrupted turn's thread, so its running indicator
    /// clears on the exact thread. `null` only when no thread is resolvable.
    TurnInterrupted {
        session_id: String,
        thread_id: Option<i64>,
    },
    /// The transcript grew between hooks (continuous tail).
    TranscriptUpdated {
        session_id: String,
        thread_ids: Vec<i64>,
    },
    /// A tool permission prompt is imminent.
    PermissionRequested {
        session_id: String,
        request_id: i64,
        tool_name: String,
        /// The tool input, serialized as JSON text, so the notice can show
        /// what the tool is about to do next to its Allow/Deny buttons.
        tool_input: String,
        /// What allowing the request would do to files on disk, when the
        /// provider stated it before asking — so the card can name the files
        /// and show the diff instead of a truncated blob of request params.
        ///
        /// Absent from the frame entirely when nothing is known, which is every
        /// request that is not a file change: the card then renders from
        /// `tool_input` alone, byte-for-byte the frame it has always been.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        file_change: Option<WireFileChangeDetail>,
        /// A directory the request also asks to be allowed to write under for
        /// the rest of the session, when the provider asked for one.
        ///
        /// A **broader** ask than `file_change`, and independent of it: that
        /// names the files this one request would touch, while this is a
        /// standing permission over a whole tree. It is present or absent on its
        /// own — including on a request that carries no `file_change` at all —
        /// so a client must render it separately rather than as another path in
        /// the change list.
        ///
        /// Absent from the frame entirely when the provider asked for no root,
        /// which is every request that is not a file change.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        grant_root: Option<String>,
    },
    /// Claude Code's `AskUserQuestion` tool is presenting a multiple-choice
    /// question; the user picks an option in the TUI.
    QuestionAsked {
        session_id: String,
        request_id: i64,
        /// The in-flight turn's thread, so the browser only shows the question
        /// card on the thread it belongs to.
        thread_id: i64,
        /// The raw `{"questions":[…]}` tool input, serialized as JSON text, so
        /// the browser can render the question card.
        tool_input: String,
    },
    /// A previously-requested tool permission was resolved.
    PermissionResolved { session_id: String, request_id: i64 },
    /// A freshly-spawned session failed to come up before it ever registered.
    SpawnFailed {
        session_id: String,
        pane_token: String,
    },
    /// A chunk of the in-flight turn's assistant message, streamed live.
    AssistantStreaming {
        session_id: String,
        thread_id: i64,
        message_id: String,
        index: u32,
        /// `final` is a Rust keyword, so the field is `is_final` here while the
        /// wire key stays `final` (the client accumulates until it is `true`).
        #[serde(rename = "final")]
        is_final: bool,
        delta: String,
    },
    /// A subagent started running (its own transcript is never tailed, so this
    /// is the only live signal): an `Agent`/`Task` tool call inside a turn, or
    /// the background agent Claude Code forks for a slash command's skill, which
    /// arrives with NO turn in flight at all — so a client must not scope an
    /// entry to the current turn. Correlated to its `subagent_finished` by
    /// `tool_use_id`.
    SubagentStarted {
        session_id: String,
        /// The thread that launched the subagent, so the client keeps that
        /// thread's running indicator lit — and its unread badge suppressed —
        /// until the subagent finishes, which for a background subagent outlives
        /// the launching turn. `subagent_finished` carries no thread; the client
        /// maps its `tool_use_id` back to this entry's thread.
        thread_id: i64,
        tool_use_id: String,
        subagent_type: Option<String>,
        description: Option<String>,
        /// Whether the launch runs in the background — `run_in_background: true`
        /// for a tool call, and always true for a forked skill. A background
        /// subagent outlives the launching turn (the client must not sweep it at
        /// turn end) and is finished by its completion notification.
        background: bool,
    },
    /// A subagent finished running, correlated to its `subagent_started` by
    /// `tool_use_id`.
    SubagentFinished {
        session_id: String,
        tool_use_id: String,
    },
    /// The latest usage snapshot for a session: selected model, context-window
    /// usage, the account's rate limits, and cost. A "latest value" keyed by
    /// `session_id` (each snapshot supersedes the last), not an append.
    StatusUpdated {
        session_id: String,
        snapshot: WireStatusSnapshot,
    },
    /// An asynchronous repository clone (`POST /api/repositories/clone`)
    /// finished: the working tree now exists at `destination_path`.
    ///
    /// The one event family on this stream carrying **no `session_id`** —
    /// cloning is a workspace-level command with no session behind it. Clients
    /// key it by `repo_owner`/`repo_name` and refetch the PR and repository
    /// lists, whose `has_local_clone` / clone rows it flips.
    RepositoryCloneCompleted {
        repo_owner: String,
        repo_name: String,
        clone_root: String,
        /// `<clone_root>/<repo_name>`. The clone is renamed onto this path
        /// atomically, so its existence means a finished clone, never a partial
        /// one.
        destination_path: String,
    },
    /// An asynchronous repository clone failed. `destination_path` does not
    /// exist when this arrives (the clone is assembled in a temporary sibling
    /// directory, removed on failure), so retrying is the same request again.
    RepositoryCloneFailed {
        repo_owner: String,
        repo_name: String,
        clone_root: String,
        destination_path: String,
        /// Why it failed, as `gh` reported it — shown to the user verbatim.
        message: String,
    },
}

/// The wire form of a session's usage snapshot. Every field is optional (a
/// session before its provider's first usage report carries `null`s), and a
/// provider that reports usage on several frames sends one snapshot per frame,
/// each stating only what that frame said; see `delta_usecase::StatusSnapshot`
/// for the semantics.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(rename = "StatusSnapshot")]
pub struct WireStatusSnapshot {
    /// Which provider's account and session these numbers describe. The client
    /// keys the account-scoped rate limits by this, so one provider's limits
    /// are never shown for another's session.
    pub provider: WireAgentProvider,
    pub model_id: Option<String>,
    pub model_display_name: Option<String>,
    pub context_used_percentage: Option<f64>,
    pub context_window_size: Option<u64>,
    pub context_current_usage: Option<u64>,
    pub total_input_tokens: Option<u64>,
    /// The account's rate-limit windows, most significant first. `null` means
    /// this snapshot says nothing about rate limits (so the client keeps what it
    /// has); `[]` means the account has none to show.
    pub rate_limits: Option<Vec<WireRateLimitWindow>>,
    pub total_cost_usd: Option<f64>,
    pub current_dir: Option<String>,
}

/// The wire form of one rate-limit window, identified by its duration rather
/// than by a provider-specific name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(rename = "RateLimitWindow")]
pub struct WireRateLimitWindow {
    /// How long the window is, in seconds — what the client labels the row
    /// from. `null` when the provider reported a window without its length.
    pub duration_seconds: Option<i64>,
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds at which the window resets.
    pub resets_at: Option<i64>,
}

impl From<RateLimitWindow> for WireRateLimitWindow {
    fn from(window: RateLimitWindow) -> Self {
        Self {
            duration_seconds: window.duration_seconds,
            used_percentage: window.used_percentage,
            resets_at: window.resets_at,
        }
    }
}

impl From<StatusSnapshot> for WireStatusSnapshot {
    fn from(snapshot: StatusSnapshot) -> Self {
        Self {
            provider: snapshot.provider.into(),
            model_id: snapshot.model_id,
            model_display_name: snapshot.model_display_name,
            context_used_percentage: snapshot.context_used_percentage,
            context_window_size: snapshot.context_window_size,
            context_current_usage: snapshot.context_current_usage,
            total_input_tokens: snapshot.total_input_tokens,
            rate_limits: snapshot
                .rate_limits
                .map(|windows| windows.into_iter().map(WireRateLimitWindow::from).collect()),
            total_cost_usd: snapshot.total_cost_usd,
            current_dir: snapshot.current_dir,
        }
    }
}

impl From<SessionEvent> for WireSessionEvent {
    fn from(event: SessionEvent) -> Self {
        match event {
            SessionEvent::SessionRegistered { session_id } => Self::SessionRegistered {
                session_id: session_id.0,
            },
            SessionEvent::SessionOpened { session_id } => Self::SessionOpened {
                session_id: session_id.0,
            },
            SessionEvent::SessionClosed { session_id } => Self::SessionClosed {
                session_id: session_id.0,
            },
            SessionEvent::SendDispatched {
                session_id,
                send_id,
            } => Self::SendDispatched {
                session_id: session_id.0,
                send_id,
            },
            SessionEvent::SendParked {
                session_id,
                send_id,
                text,
            } => Self::SendParked {
                session_id: session_id.0,
                send_id,
                text,
            },
            SessionEvent::TurnStarted {
                session_id,
                send_id,
                thread_id,
                matched_uuid,
            } => Self::TurnStarted {
                session_id: session_id.0,
                send_id,
                thread_id: thread_id.0,
                matched_uuid: matched_uuid.0,
            },
            SessionEvent::ExternalInput { session_id, prompt } => Self::ExternalInput {
                session_id: session_id.0,
                prompt,
            },
            SessionEvent::TurnCompleted {
                session_id,
                thread_id,
                stop_reason,
            } => Self::TurnCompleted {
                session_id: session_id.0,
                thread_id: thread_id.map(|id| id.0),
                stop_reason,
            },
            SessionEvent::TurnInterrupted {
                session_id,
                thread_id,
            } => Self::TurnInterrupted {
                session_id: session_id.0,
                thread_id: thread_id.map(|id| id.0),
            },
            SessionEvent::TranscriptUpdated {
                session_id,
                thread_ids,
            } => Self::TranscriptUpdated {
                session_id: session_id.0,
                thread_ids: thread_ids.into_iter().map(|id| id.0).collect(),
            },
            SessionEvent::PermissionRequested {
                session_id,
                request_id,
                tool_name,
                tool_input_json,
                file_change,
                grant_root,
            } => Self::PermissionRequested {
                session_id: session_id.0,
                request_id,
                tool_name,
                tool_input: tool_input_json,
                file_change: file_change.as_ref().map(WireFileChangeDetail::from),
                grant_root,
            },
            SessionEvent::QuestionAsked {
                session_id,
                request_id,
                thread_id,
                tool_input_json,
            } => Self::QuestionAsked {
                session_id: session_id.0,
                request_id,
                thread_id: thread_id.0,
                tool_input: tool_input_json,
            },
            SessionEvent::PermissionResolved {
                session_id,
                request_id,
            } => Self::PermissionResolved {
                session_id: session_id.0,
                request_id,
            },
            SessionEvent::SpawnFailed {
                session_id,
                pane_token,
            } => Self::SpawnFailed {
                session_id: session_id.0,
                pane_token,
            },
            SessionEvent::AssistantStreaming {
                session_id,
                thread_id,
                message_id,
                index,
                final_,
                delta,
            } => Self::AssistantStreaming {
                session_id: session_id.0,
                thread_id: thread_id.0,
                message_id,
                index,
                is_final: final_,
                delta,
            },
            SessionEvent::SubagentStarted {
                session_id,
                thread_id,
                tool_use_id,
                subagent_type,
                description,
                background,
            } => Self::SubagentStarted {
                session_id: session_id.0,
                thread_id: thread_id.0,
                tool_use_id,
                subagent_type,
                description,
                background,
            },
            SessionEvent::SubagentFinished {
                session_id,
                tool_use_id,
            } => Self::SubagentFinished {
                session_id: session_id.0,
                tool_use_id,
            },
            SessionEvent::StatusUpdated {
                session_id,
                snapshot,
            } => Self::StatusUpdated {
                session_id: session_id.0,
                snapshot: WireStatusSnapshot::from(snapshot),
            },
            SessionEvent::RepositoryCloneCompleted {
                repo_owner,
                repo_name,
                clone_root,
                destination_path,
            } => Self::RepositoryCloneCompleted {
                repo_owner,
                repo_name,
                clone_root,
                destination_path,
            },
            SessionEvent::RepositoryCloneFailed {
                repo_owner,
                repo_name,
                clone_root,
                destination_path,
                message,
            } => Self::RepositoryCloneFailed {
                repo_owner,
                repo_name,
                clone_root,
                destination_path,
                message,
            },
        }
    }
}

/// Every `kind` discriminant, in declaration order, as serde puts it on the
/// wire.
///
/// Derived by serializing one sample of each variant, so the strings come from
/// the same serde attributes that produce the actual frames — there is no
/// second, hand-maintained list to drift.
pub fn event_kinds() -> Vec<String> {
    sample_events()
        .iter()
        .map(|event| {
            serde_json::to_value(event)
                .expect("wire event serializes")
                .get("kind")
                .and_then(|kind| kind.as_str())
                .expect("wire event carries a string `kind` tag")
                .to_owned()
        })
        .collect()
}

/// One sample of every variant, in declaration order.
fn sample_events() -> Vec<WireSessionEvent> {
    // Exhaustiveness guard: adding a `WireSessionEvent` variant fails this
    // match until the new variant also gets a sample below.
    fn covered(event: &WireSessionEvent) {
        match event {
            WireSessionEvent::SessionRegistered { .. }
            | WireSessionEvent::SessionOpened { .. }
            | WireSessionEvent::SessionClosed { .. }
            | WireSessionEvent::SendDispatched { .. }
            | WireSessionEvent::SendParked { .. }
            | WireSessionEvent::TurnStarted { .. }
            | WireSessionEvent::ExternalInput { .. }
            | WireSessionEvent::TurnCompleted { .. }
            | WireSessionEvent::TurnInterrupted { .. }
            | WireSessionEvent::TranscriptUpdated { .. }
            | WireSessionEvent::PermissionRequested { .. }
            | WireSessionEvent::QuestionAsked { .. }
            | WireSessionEvent::PermissionResolved { .. }
            | WireSessionEvent::SpawnFailed { .. }
            | WireSessionEvent::AssistantStreaming { .. }
            | WireSessionEvent::SubagentStarted { .. }
            | WireSessionEvent::SubagentFinished { .. }
            | WireSessionEvent::StatusUpdated { .. }
            | WireSessionEvent::RepositoryCloneCompleted { .. }
            | WireSessionEvent::RepositoryCloneFailed { .. } => {}
        }
    }

    let session_id = || "sess-sample".to_owned();
    let samples = vec![
        WireSessionEvent::SessionRegistered {
            session_id: session_id(),
        },
        WireSessionEvent::SessionOpened {
            session_id: session_id(),
        },
        WireSessionEvent::SessionClosed {
            session_id: session_id(),
        },
        WireSessionEvent::SendDispatched {
            session_id: session_id(),
            send_id: 1,
        },
        WireSessionEvent::SendParked {
            session_id: session_id(),
            send_id: 1,
            text: "never delivered".to_owned(),
        },
        WireSessionEvent::TurnStarted {
            session_id: session_id(),
            send_id: 1,
            thread_id: 1,
            matched_uuid: "uuid-sample".to_owned(),
        },
        WireSessionEvent::ExternalInput {
            session_id: session_id(),
            prompt: "prompt".to_owned(),
        },
        WireSessionEvent::TurnCompleted {
            session_id: session_id(),
            thread_id: Some(1),
            stop_reason: None,
        },
        WireSessionEvent::TurnInterrupted {
            session_id: session_id(),
            thread_id: Some(1),
        },
        WireSessionEvent::TranscriptUpdated {
            session_id: session_id(),
            thread_ids: vec![1],
        },
        WireSessionEvent::PermissionRequested {
            session_id: session_id(),
            request_id: 1,
            tool_name: "Bash".to_owned(),
            tool_input: "{\"command\":\"ls\"}".to_owned(),
            file_change: None,
            grant_root: None,
        },
        WireSessionEvent::QuestionAsked {
            session_id: session_id(),
            request_id: 1,
            thread_id: 1,
            tool_input: "{\"questions\":[]}".to_owned(),
        },
        WireSessionEvent::PermissionResolved {
            session_id: session_id(),
            request_id: 1,
        },
        WireSessionEvent::SpawnFailed {
            session_id: session_id(),
            pane_token: "delta-sample".to_owned(),
        },
        WireSessionEvent::AssistantStreaming {
            session_id: session_id(),
            thread_id: 1,
            message_id: "msg-sample".to_owned(),
            index: 0,
            is_final: false,
            delta: "chunk".to_owned(),
        },
        WireSessionEvent::SubagentStarted {
            session_id: session_id(),
            thread_id: 1,
            tool_use_id: "toolu-sample".to_owned(),
            subagent_type: Some("general-purpose".to_owned()),
            description: Some("Run ls".to_owned()),
            background: false,
        },
        WireSessionEvent::SubagentFinished {
            session_id: session_id(),
            tool_use_id: "toolu-sample".to_owned(),
        },
        WireSessionEvent::StatusUpdated {
            session_id: session_id(),
            // A populated sample rather than an empty one: the exhaustiveness
            // guard is what proves a new snapshot field reaches the generated
            // bindings, and an all-`null` sample would let a field be added
            // without its shape ever being exercised. The rate-limit window
            // carries its duration, which is the window's identity on the wire.
            snapshot: WireStatusSnapshot {
                provider: WireAgentProvider::Claude,
                model_id: Some("model-sample".to_owned()),
                model_display_name: Some("Model Sample".to_owned()),
                context_used_percentage: Some(42.5),
                context_window_size: Some(200_000),
                context_current_usage: Some(85_000),
                total_input_tokens: Some(90_000),
                rate_limits: Some(vec![WireRateLimitWindow {
                    duration_seconds: Some(5 * 60 * 60),
                    used_percentage: Some(12.0),
                    resets_at: Some(1_700_000_000),
                }]),
                total_cost_usd: Some(0.1234),
                current_dir: Some("/work".to_owned()),
            },
        },
        WireSessionEvent::RepositoryCloneCompleted {
            repo_owner: "x7c1".to_owned(),
            repo_name: "delta".to_owned(),
            clone_root: "/home/dev/projects".to_owned(),
            destination_path: "/home/dev/projects/delta".to_owned(),
        },
        WireSessionEvent::RepositoryCloneFailed {
            repo_owner: "x7c1".to_owned(),
            repo_name: "delta".to_owned(),
            clone_root: "/home/dev/projects".to_owned(),
            destination_path: "/home/dev/projects/delta".to_owned(),
            message: "could not resolve host github.com".to_owned(),
        },
    ];
    for event in &samples {
        covered(event);
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_change::{WireFileChange, WireFileChangeKind};

    use delta_model::{AgentProvider, MessageUuid, SessionId, ThreadId};

    fn json(event: &WireSessionEvent) -> serde_json::Value {
        serde_json::to_value(event).unwrap()
    }

    #[test]
    fn open_and_closed_serialize_as_id_routed_tagged_events() {
        assert_eq!(
            json(&WireSessionEvent::SessionOpened {
                session_id: "sess-1".into(),
            }),
            serde_json::json!({ "kind": "session_opened", "session_id": "sess-1" }),
        );
        assert_eq!(
            json(&WireSessionEvent::SessionClosed {
                session_id: "sess-1".into(),
            }),
            serde_json::json!({ "kind": "session_closed", "session_id": "sess-1" }),
        );
    }

    #[test]
    fn registered_keeps_its_wire_shape() {
        assert_eq!(
            json(&WireSessionEvent::SessionRegistered {
                session_id: "sess-1".into(),
            }),
            serde_json::json!({ "kind": "session_registered", "session_id": "sess-1" }),
        );
    }

    #[test]
    fn turn_interrupted_serializes_as_id_routed_tagged_event() {
        assert_eq!(
            json(&WireSessionEvent::TurnInterrupted {
                session_id: "sess-1".into(),
                thread_id: Some(4),
            }),
            serde_json::json!({
                "kind": "turn_interrupted",
                "session_id": "sess-1",
                "thread_id": 4,
            }),
        );
    }

    #[test]
    fn spawn_failed_serializes_with_id_and_pane_token() {
        assert_eq!(
            json(&WireSessionEvent::SpawnFailed {
                session_id: "sess-1".into(),
                pane_token: "delta-1".into(),
            }),
            serde_json::json!({
                "kind": "spawn_failed",
                "session_id": "sess-1",
                "pane_token": "delta-1",
            }),
        );
    }

    /// The Claude path (and every non-file-change request): with no detail to
    /// carry, the frame is byte-for-byte the one it has always been — the
    /// `file_change` key is absent, not `null`.
    #[test]
    fn permission_requested_and_resolved_serialize_as_tagged_events() {
        assert_eq!(
            json(&WireSessionEvent::PermissionRequested {
                session_id: "sess-1".into(),
                request_id: 7,
                tool_name: "Bash".into(),
                tool_input: "{\"command\":\"rm -i x\"}".into(),
                file_change: None,
                grant_root: None,
            }),
            serde_json::json!({
                "kind": "permission_requested",
                "session_id": "sess-1",
                "request_id": 7,
                "tool_name": "Bash",
                "tool_input": "{\"command\":\"rm -i x\"}",
            }),
        );
        assert_eq!(
            json(&WireSessionEvent::PermissionResolved {
                session_id: "sess-1".into(),
                request_id: 7,
            }),
            serde_json::json!({
                "kind": "permission_resolved",
                "session_id": "sess-1",
                "request_id": 7,
            }),
        );
    }

    /// A file-change request carries the detail the card renders: the affected
    /// paths, how each changes, the diff, and the provider's stated reason.
    #[test]
    fn a_file_change_permission_carries_its_paths_kinds_and_diffs() {
        assert_eq!(
            json(&WireSessionEvent::PermissionRequested {
                session_id: "sess-1".into(),
                request_id: 7,
                tool_name: "file_change".into(),
                tool_input: "{\"itemId\":\"fc_1\"}".into(),
                file_change: Some(WireFileChangeDetail {
                    changes: vec![WireFileChange {
                        path: "src/lib.rs".into(),
                        kind: Some(WireFileChangeKind::Update),
                        diff: "@@ -1 +1 @@".into(),
                    }],
                    reason: Some("write access".into()),
                }),
                grant_root: None,
            }),
            serde_json::json!({
                "kind": "permission_requested",
                "session_id": "sess-1",
                "request_id": 7,
                "tool_name": "file_change",
                "tool_input": "{\"itemId\":\"fc_1\"}",
                "file_change": {
                    "changes": [
                        { "path": "src/lib.rs", "kind": "update", "diff": "@@ -1 +1 @@" },
                    ],
                    "reason": "write access",
                },
            }),
        );
    }

    /// A `grant_root` rides the frame on its own, with no `file_change` beside
    /// it — the shape an approval takes when its change set could not be
    /// correlated. It must still cross: it is the broadest thing the dialog
    /// grants (writes anywhere under that root for the rest of the session), so
    /// a frame that dropped it would understate the request precisely where the
    /// client has nothing else to show.
    #[test]
    fn a_grant_root_crosses_even_with_no_change_set_beside_it() {
        assert_eq!(
            json(&WireSessionEvent::PermissionRequested {
                session_id: "sess-1".into(),
                request_id: 8,
                tool_name: "file_change".into(),
                tool_input: "{\"itemId\":\"fc_2\"}".into(),
                file_change: None,
                grant_root: Some("/repo".into()),
            }),
            serde_json::json!({
                "kind": "permission_requested",
                "session_id": "sess-1",
                "request_id": 8,
                "tool_name": "file_change",
                "tool_input": "{\"itemId\":\"fc_2\"}",
                "grant_root": "/repo",
            }),
        );
    }

    #[test]
    fn question_asked_serializes_as_a_tagged_event_carrying_raw_tool_input() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::QuestionAsked {
                session_id: SessionId::from("sess-1"),
                request_id: 9,
                thread_id: ThreadId(3),
                tool_input_json: "{\"questions\":[{\"header\":\"Pick\"}]}".to_owned(),
            })),
            serde_json::json!({
                "kind": "question_asked",
                "session_id": "sess-1",
                "request_id": 9,
                "thread_id": 3,
                "tool_input": "{\"questions\":[{\"header\":\"Pick\"}]}",
            }),
        );
    }

    #[test]
    fn send_dispatched_serializes_with_id_and_send_id() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::SendDispatched {
                session_id: SessionId::from("sess-1"),
                send_id: 42,
            })),
            serde_json::json!({
                "kind": "send_dispatched",
                "session_id": "sess-1",
                "send_id": 42,
            }),
        );
    }

    #[test]
    fn send_parked_serializes_with_the_undelivered_text() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::SendParked {
                session_id: SessionId::from("sess-1"),
                send_id: 42,
                text: "read this\n/home/dev/pictures/shot.png".to_owned(),
            })),
            serde_json::json!({
                "kind": "send_parked",
                "session_id": "sess-1",
                "send_id": 42,
                "text": "read this\n/home/dev/pictures/shot.png",
            }),
        );
    }

    #[test]
    fn turn_events_keep_their_payload_fields_on_the_wire() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::TurnStarted {
                session_id: SessionId::from("sess-1"),
                send_id: 42,
                thread_id: ThreadId(2),
                matched_uuid: MessageUuid::from("uuid-1"),
            })),
            serde_json::json!({
                "kind": "turn_started",
                "session_id": "sess-1",
                "send_id": 42,
                "thread_id": 2,
                "matched_uuid": "uuid-1",
            }),
        );
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::TurnCompleted {
                session_id: SessionId::from("sess-1"),
                thread_id: Some(ThreadId(2)),
                stop_reason: None,
            })),
            serde_json::json!({
                "kind": "turn_completed",
                "session_id": "sess-1",
                "thread_id": 2,
                "stop_reason": null,
            }),
        );
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::TranscriptUpdated {
                session_id: SessionId::from("sess-1"),
                thread_ids: vec![ThreadId(3), ThreadId(5)],
            })),
            serde_json::json!({
                "kind": "transcript_updated",
                "session_id": "sess-1",
                "thread_ids": [3, 5],
            }),
        );
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::ExternalInput {
                session_id: SessionId::from("sess-1"),
                prompt: "typed in pane".into(),
            })),
            serde_json::json!({
                "kind": "external_input",
                "session_id": "sess-1",
                "prompt": "typed in pane",
            }),
        );
    }

    #[test]
    fn event_kinds_lists_every_variant_in_declaration_order() {
        assert_eq!(
            event_kinds(),
            [
                "session_registered",
                "session_opened",
                "session_closed",
                "send_dispatched",
                "send_parked",
                "turn_started",
                "external_input",
                "turn_completed",
                "turn_interrupted",
                "transcript_updated",
                "permission_requested",
                "question_asked",
                "permission_resolved",
                "spawn_failed",
                "assistant_streaming",
                "subagent_started",
                "subagent_finished",
                "status_updated",
                "repository_clone_completed",
                "repository_clone_failed",
            ],
        );
    }

    #[test]
    fn a_repository_clone_event_serializes_without_a_session_id() {
        // The clone events are the exception to the "every frame names its
        // session" rule, so pin the absence: a client narrowing the union on
        // `session_id` must not find one here by accident.
        let value = json(&WireSessionEvent::from(
            SessionEvent::RepositoryCloneCompleted {
                repo_owner: "x7c1".into(),
                repo_name: "delta".into(),
                clone_root: "/home/dev/projects".into(),
                destination_path: "/home/dev/projects/delta".into(),
            },
        ));
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "repository_clone_completed",
                "repo_owner": "x7c1",
                "repo_name": "delta",
                "clone_root": "/home/dev/projects",
                "destination_path": "/home/dev/projects/delta",
            }),
        );
    }

    #[test]
    fn a_failed_repository_clone_carries_the_reason() {
        assert_eq!(
            json(&WireSessionEvent::from(
                SessionEvent::RepositoryCloneFailed {
                    repo_owner: "x7c1".into(),
                    repo_name: "delta".into(),
                    clone_root: "/home/dev/projects".into(),
                    destination_path: "/home/dev/projects/delta".into(),
                    message: "repository not found".into(),
                }
            )),
            serde_json::json!({
                "kind": "repository_clone_failed",
                "repo_owner": "x7c1",
                "repo_name": "delta",
                "clone_root": "/home/dev/projects",
                "destination_path": "/home/dev/projects/delta",
                "message": "repository not found",
            }),
        );
    }

    #[test]
    fn assistant_streaming_serializes_with_its_chunk_payload() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::AssistantStreaming {
                session_id: SessionId::from("sess-1"),
                thread_id: ThreadId(3),
                message_id: "msg-7".into(),
                index: 2,
                final_: true,
                delta: "hello".into(),
            })),
            serde_json::json!({
                "kind": "assistant_streaming",
                "session_id": "sess-1",
                "thread_id": 3,
                "message_id": "msg-7",
                "index": 2,
                "final": true,
                "delta": "hello",
            }),
        );
    }

    #[test]
    fn subagent_started_serializes_with_its_correlation_and_display_fields() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::SubagentStarted {
                session_id: SessionId::from("sess-1"),
                thread_id: ThreadId(2),
                tool_use_id: "toolu_01".into(),
                subagent_type: Some("general-purpose".into()),
                description: Some("Run ls and count entries".into()),
                background: false,
            })),
            serde_json::json!({
                "kind": "subagent_started",
                "session_id": "sess-1",
                "thread_id": 2,
                "tool_use_id": "toolu_01",
                "subagent_type": "general-purpose",
                "description": "Run ls and count entries",
                "background": false,
            }),
        );
    }

    #[test]
    fn subagent_started_carries_the_background_flag_when_launched_in_background() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::SubagentStarted {
                session_id: SessionId::from("sess-1"),
                thread_id: ThreadId(2),
                tool_use_id: "toolu_01".into(),
                subagent_type: Some("general-purpose".into()),
                description: Some("Long crawl".into()),
                background: true,
            })),
            serde_json::json!({
                "kind": "subagent_started",
                "session_id": "sess-1",
                "thread_id": 2,
                "tool_use_id": "toolu_01",
                "subagent_type": "general-purpose",
                "description": "Long crawl",
                "background": true,
            }),
        );
    }

    #[test]
    fn subagent_started_keeps_null_display_fields_when_absent() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::SubagentStarted {
                session_id: SessionId::from("sess-1"),
                thread_id: ThreadId(2),
                tool_use_id: "toolu_01".into(),
                subagent_type: None,
                description: None,
                background: false,
            })),
            serde_json::json!({
                "kind": "subagent_started",
                "session_id": "sess-1",
                "thread_id": 2,
                "tool_use_id": "toolu_01",
                "subagent_type": null,
                "description": null,
                "background": false,
            }),
        );
    }

    #[test]
    fn status_updated_serializes_its_snapshot_with_duration_identified_rate_limits() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::StatusUpdated {
                session_id: SessionId::from("sess-1"),
                snapshot: StatusSnapshot {
                    model_id: Some("claude-opus-4".to_owned()),
                    model_display_name: Some("Opus 4".to_owned()),
                    context_used_percentage: Some(42.5),
                    context_window_size: Some(200_000),
                    context_current_usage: Some(85_000),
                    total_input_tokens: Some(90_000),
                    rate_limits: Some(vec![
                        RateLimitWindow {
                            duration_seconds: Some(5 * 60 * 60),
                            used_percentage: Some(12.0),
                            resets_at: Some(1_700_000_000),
                        },
                        RateLimitWindow {
                            duration_seconds: Some(7 * 24 * 60 * 60),
                            used_percentage: Some(3.5),
                            resets_at: Some(1_700_500_000),
                        },
                    ]),
                    total_cost_usd: Some(0.1234),
                    current_dir: Some("/work".to_owned()),
                    ..StatusSnapshot::new(AgentProvider::Claude)
                },
            })),
            serde_json::json!({
                "kind": "status_updated",
                "session_id": "sess-1",
                "snapshot": {
                    "provider": "claude",
                    "model_id": "claude-opus-4",
                    "model_display_name": "Opus 4",
                    "context_used_percentage": 42.5,
                    "context_window_size": 200_000,
                    "context_current_usage": 85_000,
                    "total_input_tokens": 90_000,
                    "rate_limits": [
                        { "duration_seconds": 18_000, "used_percentage": 12.0, "resets_at": 1_700_000_000 },
                        { "duration_seconds": 604_800, "used_percentage": 3.5, "resets_at": 1_700_500_000 },
                    ],
                    "total_cost_usd": 0.1234,
                    "current_dir": "/work",
                },
            }),
        );
    }

    #[test]
    fn status_updated_pre_api_snapshot_carries_nulls_and_absent_rate_limits() {
        // Before the first API response, rate limits and context usage are not
        // yet known; the snapshot still serializes with explicit nulls.
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::StatusUpdated {
                session_id: SessionId::from("sess-1"),
                snapshot: StatusSnapshot {
                    model_display_name: Some("Opus 4".to_owned()),
                    ..StatusSnapshot::new(AgentProvider::Claude)
                },
            })),
            serde_json::json!({
                "kind": "status_updated",
                "session_id": "sess-1",
                "snapshot": {
                    "provider": "claude",
                    "model_id": null,
                    "model_display_name": "Opus 4",
                    "context_used_percentage": null,
                    "context_window_size": null,
                    "context_current_usage": null,
                    "total_input_tokens": null,
                    "rate_limits": null,
                    "total_cost_usd": null,
                    "current_dir": null,
                },
            }),
        );
    }

    #[test]
    fn a_non_claude_snapshot_names_its_own_provider_and_may_state_only_rate_limits() {
        // The account-scoped Codex frame: it says nothing about context usage,
        // and its windows are identified by the duration the server reported.
        // The provider tag is what keeps these limits off a Claude session.
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::StatusUpdated {
                session_id: SessionId::from("sess-1"),
                snapshot: StatusSnapshot {
                    rate_limits: Some(vec![RateLimitWindow {
                        duration_seconds: Some(300 * 60),
                        used_percentage: Some(21.0),
                        resets_at: None,
                    }]),
                    ..StatusSnapshot::new(AgentProvider::Codex)
                },
            })),
            serde_json::json!({
                "kind": "status_updated",
                "session_id": "sess-1",
                "snapshot": {
                    "provider": "codex",
                    "model_id": null,
                    "model_display_name": null,
                    "context_used_percentage": null,
                    "context_window_size": null,
                    "context_current_usage": null,
                    "total_input_tokens": null,
                    "rate_limits": [
                        { "duration_seconds": 18_000, "used_percentage": 21.0, "resets_at": null },
                    ],
                    "total_cost_usd": null,
                    "current_dir": null,
                },
            }),
        );
    }

    #[test]
    fn subagent_finished_serializes_with_its_correlation_id() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::SubagentFinished {
                session_id: SessionId::from("sess-1"),
                tool_use_id: "toolu_01".into(),
            })),
            serde_json::json!({
                "kind": "subagent_finished",
                "session_id": "sess-1",
                "tool_use_id": "toolu_01",
            }),
        );
    }
}
