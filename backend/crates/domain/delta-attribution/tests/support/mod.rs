//! Shared helpers for the attribution test binaries.
//!
//! Each file under `tests/` is its own crate and compiles its own copy of
//! this module, using a different subset of it — hence the dead-code allow.
#![allow(dead_code)]

pub mod corpus;

use delta_model::{ContentBlock, Message, MessageUuid, Role, SessionId, ThreadId};

use delta_attribution::{Attributed, OutstandingSend, TranscriptMessage};

/// The session id every test folds under.
pub fn session() -> SessionId {
    SessionId::from("sess-test")
}

pub const MAIN: ThreadId = ThreadId(1);
pub const CHILD: ThreadId = ThreadId(2);

// --- Transcript line builders -------------------------------------------------

pub fn user_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        uuid: MessageUuid::from(uuid),
        role: Role::User,
        linear_parent_uuid: None,
        prompt_id: None,
        content: vec![ContentBlock::Text { text: text.into() }],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        seq: 0,
        is_queued_command: false,
        is_api_error: false,
    }
}

pub fn assistant_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        role: Role::Assistant,
        ..user_line(uuid, text)
    }
}

/// A synthetic `isApiErrorMessage` assistant line: a turn that ended on an API
/// error (a usage/session limit, a rate limit, or any other API failure)
/// instead of completing normally. It fires no `Stop` hook and writes no
/// interrupt marker, so the flag is its only turn-end signal.
pub fn api_error_line(uuid: &str) -> TranscriptMessage {
    TranscriptMessage {
        is_api_error: true,
        ..assistant_line(uuid, "You've hit your session limit")
    }
}

/// A `queued_command` attachment line: a prompt the user composed while a
/// turn was in flight, surfaced by the parser as a flagged user line.
/// LEGACY FORMAT — written only by older claude versions; current claude
/// replays queued prompts as plain user lines (see the queued-prompt drift
/// note in docs/guides/development.md).
pub fn queued_command_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        is_queued_command: true,
        ..user_line(uuid, text)
    }
}

/// The interrupt marker Claude writes when the user aborts the in-flight
/// turn: a `role: user` line that belongs to the aborted turn.
pub fn interrupt_line(uuid: &str) -> TranscriptMessage {
    user_line(uuid, "[Request interrupted by user]")
}

/// A harness-injected `<task-notification>`: a background-task completion that
/// current claude delivers as a plain `role: user` line (NOT a legacy
/// `queued_command` attachment), so it carries no `is_queued_command` flag.
/// Carries no `<tool-use-id>`, so it exercises the unknown-launch fallback
/// (inherit `carry_thread`).
pub fn task_notification_line(uuid: &str) -> TranscriptMessage {
    user_line(uuid, "<task-notification>done</task-notification>")
}

/// A `<task-notification>` whose `<tool-use-id>` correlates back to the launch
/// of a background `Agent`/`Task`/`Bash` with that `tool_use_id`, in the real
/// harness-injected shape.
pub fn task_notification_line_for(uuid: &str, tool_use_id: &str) -> TranscriptMessage {
    user_line(
        uuid,
        &format!(
            "<task-notification>\n\
             <task-id>a31425032172620ed</task-id>\n\
             <tool-use-id>{tool_use_id}</tool-use-id>\n\
             <output-file>/tmp/x.output</output-file>\n\
             <status>completed</status>\n\
             <summary>Agent completed</summary>\n\
             </task-notification>"
        ),
    )
}

/// An assistant line launching a background tool call: a `ToolUse` whose input
/// carries `run_in_background: true`. The launching `tool_use_id` becomes the
/// correlation key for the later `<task-notification>`.
pub fn background_tool_use_line(uuid: &str, tool_use_id: &str) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolUse {
            id: tool_use_id.into(),
            name: "Agent".into(),
            input: serde_json::json!({
                "subagent_type": "general-purpose",
                "run_in_background": true
            }),
        }],
        ..assistant_line(uuid, "")
    }
}

/// An assistant line issuing a tool call (no author text).
pub fn tool_use_line(uuid: &str, tool_use_id: &str) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolUse {
            id: tool_use_id.into(),
            name: "Bash".into(),
            input: serde_json::Value::Null,
        }],
        ..assistant_line(uuid, "")
    }
}

/// The `tool_result` carrier: a `role: user` line with no author-written
/// text, belonging to the in-flight turn.
pub fn tool_result_line(uuid: &str, tool_use_id: &str, is_error: bool) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: serde_json::Value::Null,
            is_error,
        }],
        ..user_line(uuid, "")
    }
}

/// A harness-injected `isMeta` line (skill bodies, system reminders, ...).
pub fn meta_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        role: Role::Meta,
        ..user_line(uuid, text)
    }
}

/// Attach a `promptId` to a line, mirroring Claude's per-turn `promptId` that
/// groups the members of a slash/local-command sequence.
pub fn with_prompt_id(prompt_id: &str, line: TranscriptMessage) -> TranscriptMessage {
    TranscriptMessage {
        prompt_id: Some(delta_model::PromptId::from(prompt_id)),
        ..line
    }
}

/// The leading `<local-command-caveat>` of a slash/local-command group: Claude
/// records it as a `type: "user"` line flagged `isMeta` (so the parser already
/// classifies it `Role::Meta`). It carries the group's shared `promptId`.
pub fn local_command_caveat_line(uuid: &str, prompt_id: &str) -> TranscriptMessage {
    with_prompt_id(
        prompt_id,
        meta_line(
            uuid,
            "<local-command-caveat>Caveat: The messages below were generated by the user \
             while running local commands. DO NOT respond to these messages...</local-command-caveat>",
        ),
    )
}

/// The bare command-name member of a local-command group (e.g. `/review-pr`).
/// Claude does NOT flag it `isMeta`, so the parser leaves it `Role::User`; it is
/// the attribution layer that recognizes it by the group's shared `promptId`.
pub fn local_command_name_line(uuid: &str, prompt_id: &str, command: &str) -> TranscriptMessage {
    with_prompt_id(prompt_id, user_line(uuid, command))
}

/// A local command's captured output member. Claude does not flag it `isMeta`,
/// but the gateway parser folds it to `Role::Meta` by its `<local-command-...>`
/// content marker, so this builder mirrors that parsed role.
pub fn local_command_stdout_line(uuid: &str, prompt_id: &str) -> TranscriptMessage {
    with_prompt_id(
        prompt_id,
        meta_line(
            uuid,
            "<local-command-stdout>\nPENDING review created.\n</local-command-stdout>",
        ),
    )
}

/// A line whose kind Delta does not classify (e.g. a summary).
pub fn other_line(uuid: &str) -> TranscriptMessage {
    TranscriptMessage {
        role: Role::Other,
        content: Vec::new(),
        ..user_line(uuid, "")
    }
}

// --- Overlay builders ----------------------------------------------------------

/// A plain (trunk) send: its echo is attributed to `thread`.
pub fn send(id: i64, thread: ThreadId, text: &str) -> OutstandingSend {
    OutstandingSend {
        id,
        thread_id: thread,
        semantic_parent_uuid: None,
        text: text.into(),
    }
}

/// A branch send: its echo opens `thread` as a reply `to:` `parent`.
pub fn branch_send(id: i64, thread: ThreadId, parent: &str, text: &str) -> OutstandingSend {
    OutstandingSend {
        semantic_parent_uuid: Some(MessageUuid::from(parent)),
        ..send(id, thread, text)
    }
}

// --- Assertion helpers ----------------------------------------------------------

/// Look up an attributed message by uuid.
pub fn message<'a>(outcome: &'a Attributed, uuid: &str) -> &'a Message {
    outcome
        .messages
        .iter()
        .find(|m| m.uuid.as_str() == uuid)
        .unwrap_or_else(|| panic!("no attributed message with uuid {uuid}"))
}

/// The thread each message landed on, by uuid, in input order.
pub fn threads(outcome: &Attributed) -> Vec<(String, ThreadId)> {
    outcome
        .messages
        .iter()
        .map(|m| (m.uuid.as_str().to_owned(), m.thread_id))
        .collect()
}
