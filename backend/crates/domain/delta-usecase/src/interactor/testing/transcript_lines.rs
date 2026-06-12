//! Builders for the transcript lines the [`FakeTranscript`] is driven with.
//!
//! [`FakeTranscript`]: super::FakeTranscript

use delta_model::{ContentBlock, MessageUuid, Role};

use crate::ports::TranscriptMessage;

pub(crate) fn user_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        uuid: MessageUuid::from(uuid),
        role: Role::User,
        linear_parent_uuid: None,
        prompt_id: None,
        content: vec![ContentBlock::Text { text: text.into() }],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        // The reader assigns the real line index on read; this is a placeholder.
        seq: 0,
        is_queued_command: false,
    }
}

pub(crate) fn assistant_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        uuid: MessageUuid::from(uuid),
        role: Role::Assistant,
        linear_parent_uuid: None,
        prompt_id: None,
        content: vec![ContentBlock::Text { text: text.into() }],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        // The reader assigns the real line index on read; this is a placeholder.
        seq: 0,
        is_queued_command: false,
    }
}

/// A `queued_command` attachment line: a prompt the user composed while a turn
/// was in flight. Claude records it only as this attachment (never as a normal
/// user line), but Delta surfaces it as a user message that both displays and
/// flows through send correlation. Mirrors `user_line` apart from the flag.
pub(crate) fn queued_command_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        is_queued_command: true,
        ..user_line(uuid, text)
    }
}

/// An assistant transcript line stamped with an explicit `created_at`, so a
/// test can give different sessions distinct last-activity timestamps.
pub(crate) fn assistant_line_at(uuid: &str, text: &str, created_at: &str) -> TranscriptMessage {
    TranscriptMessage {
        created_at: Some(created_at.into()),
        ..assistant_line(uuid, text)
    }
}

/// An interrupt-marker line. Claude writes this as a `role: user` line whose
/// only text block is `[Request interrupted by user...]` when the user aborts
/// the in-flight turn; it belongs to the interrupted turn, not a new human turn.
pub(crate) fn interrupt_line(uuid: &str) -> TranscriptMessage {
    user_line(uuid, "[Request interrupted by user]")
}

/// A tool-result carrier line. Claude delivers these as `role: user` with no
/// author-written text; they belong to the in-flight turn, not a new human turn.
pub(crate) fn tool_result_line(uuid: &str, tool_use_id: &str) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: serde_json::Value::Null,
            is_error: false,
        }],
        ..user_line(uuid, "")
    }
}
