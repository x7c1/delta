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
    }
}

pub fn assistant_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        role: Role::Assistant,
        ..user_line(uuid, text)
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
pub fn task_notification_line(uuid: &str) -> TranscriptMessage {
    user_line(uuid, "<task-notification>done</task-notification>")
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
