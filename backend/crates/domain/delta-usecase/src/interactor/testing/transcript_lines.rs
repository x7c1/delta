//! Builders for the transcript lines the [`FakeTranscript`] is driven with.
//!
//! [`FakeTranscript`]: super::FakeTranscript

use delta_model::{ContentBlock, MessageUuid, PromptId, Role};

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
        is_queued_replay: false,
        is_api_error: false,
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
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
        is_queued_replay: false,
        is_api_error: false,
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
    }
}

/// A synthetic `isApiErrorMessage` assistant line: Claude writes this when a
/// turn ends on an API error (a usage/session limit, a rate limit, or any other
/// API failure) instead of completing normally. It fires no `Stop` hook and
/// writes no interrupt marker, so the flag is its only turn-end signal.
pub(crate) fn api_error_line(uuid: &str) -> TranscriptMessage {
    TranscriptMessage {
        is_api_error: true,
        ..assistant_line(uuid, "You've hit your session limit")
    }
}

/// A `queued_command` attachment line: a prompt the user composed while a turn
/// was in flight, which older claude versions record only as this attachment
/// (never as a normal user line); Delta surfaces it as a user message that
/// both displays and flows through send correlation. Mirrors `user_line`
/// apart from the flag. LEGACY FORMAT — current claude replays queued prompts
/// as plain user lines instead (see the queued-prompt drift note in
/// docs/guides/development.md); kept because old transcripts are still
/// resumed and viewed.
pub(crate) fn queued_command_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        is_queued_command: true,
        ..user_line(uuid, text)
    }
}

/// A modern queued-prompt REPLAY: an ordinary `type: "user"` line whose
/// `promptSource` was `"queued"`. Claude Code writes this when a prompt the
/// user submitted while a turn was in flight drains from the CLI's internal
/// input queue. Distinct from [`queued_command_line`] — the flags are
/// independent, and only the *replay* shape's flag guards the compact-group
/// exclusion in attribution. Callers that need to reproduce the post-compact
/// promptId collision should additionally stamp the group's `promptId` on
/// the returned line (via [`with_prompt_id`]).
pub(crate) fn queued_replay_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        is_queued_replay: true,
        ..user_line(uuid, text)
    }
}

/// Stamp a `promptId` onto an existing transcript line — mirrors Claude Code's
/// per-turn `promptId` that groups the members of a slash/local-command
/// sequence (and, in the post-compact edge case, catches a queued-prompt
/// replay under the same id).
pub(crate) fn with_prompt_id(prompt_id: &str, line: TranscriptMessage) -> TranscriptMessage {
    TranscriptMessage {
        prompt_id: Some(PromptId::from(prompt_id)),
        ..line
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

/// An assistant line launching a background tool call: a `ToolUse` whose input
/// carries `run_in_background: true`. The launching `tool_use_id` becomes the
/// correlation key for the later `<task-notification>`.
pub(crate) fn background_tool_use_line(uuid: &str, tool_use_id: &str) -> TranscriptMessage {
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

/// An assistant line launching an `Agent`/`Task` tool call in the modern shape:
/// a `ToolUse` with no `run_in_background` key. Modern Claude Code dropped that
/// key from the schema and made `Agent`/`Task` calls async by default, so this
/// matches what production now writes to the transcript — the predicate
/// [`delta_attribution::claude_format::launches_in_background`] classifies it
/// as background. Use [`foreground_agent_tool_use_line`] for explicit
/// `run_in_background: false` foreground semantics.
pub(crate) fn agent_tool_use_line(
    uuid: &str,
    tool_use_id: &str,
    tool_name: &str,
    subagent_type: &str,
    description: &str,
) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolUse {
            id: tool_use_id.into(),
            name: tool_name.into(),
            input: serde_json::json!({
                "subagent_type": subagent_type,
                "description": description,
            }),
        }],
        ..assistant_line(uuid, "")
    }
}

/// An assistant line launching a foreground `Agent`/`Task` tool call: a
/// `ToolUse` whose input pins `run_in_background: false`. The explicit flag is
/// what keeps the call foreground under the modern async-by-default semantics
/// of [`delta_attribution::claude_format::launches_in_background`], so the
/// matching `PostToolUse(Agent)` closes the indicator window (the foreground
/// subagent lifecycle).
pub(crate) fn foreground_agent_tool_use_line(
    uuid: &str,
    tool_use_id: &str,
    tool_name: &str,
    subagent_type: &str,
    description: &str,
) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolUse {
            id: tool_use_id.into(),
            name: tool_name.into(),
            input: serde_json::json!({
                "subagent_type": subagent_type,
                "description": description,
                "run_in_background": false,
            }),
        }],
        ..assistant_line(uuid, "")
    }
}

/// An assistant line containing an arbitrary `Bash` tool call. Used to assert
/// that a non-subagent tool_use does not light the indicator.
pub(crate) fn bash_tool_use_line(uuid: &str, tool_use_id: &str) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolUse {
            id: tool_use_id.into(),
            name: "Bash".into(),
            input: serde_json::json!({ "command": "ls" }),
        }],
        ..assistant_line(uuid, "")
    }
}

/// The synthetic user line Claude Code writes when `/compact` runs, carrying
/// the previous-conversation summary. Flagged `isCompactSummary`, so the
/// parser classifies it `Role::CompactSummary`; attribution emits
/// `Effect::AutoCompactFinished` so the sync layer can re-type any send
/// stuck behind the compaction.
pub(crate) fn compact_summary_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        role: Role::CompactSummary,
        ..user_line(uuid, text)
    }
}

/// The leading `<local-command-caveat>` member of a slash/local-command group
/// (e.g. when the user runs `/review-pr`). Claude records it as a `type: "user"`
/// line flagged `isMeta` (so the parser classifies it `Role::Meta`) and stamps
/// it with the group's shared `promptId`.
pub(crate) fn local_command_caveat_line(uuid: &str, prompt_id: &str) -> TranscriptMessage {
    TranscriptMessage {
        role: Role::Meta,
        prompt_id: Some(PromptId::from(prompt_id)),
        ..user_line(
            uuid,
            "<local-command-caveat>Caveat: The messages below were generated by the user \
             while running local commands. DO NOT respond to these messages...</local-command-caveat>",
        )
    }
}

/// The bare command-name member of a local-command group (e.g. `/review-pr`):
/// Claude does NOT flag it `isMeta`, so the parser leaves it `Role::User`. It
/// carries the group's shared `promptId`, by which attribution recognizes it.
pub(crate) fn local_command_name_line(
    uuid: &str,
    prompt_id: &str,
    command: &str,
) -> TranscriptMessage {
    TranscriptMessage {
        prompt_id: Some(PromptId::from(prompt_id)),
        ..user_line(uuid, command)
    }
}

/// A local command's captured `<local-command-stdout>` output: Claude does not
/// flag it `isMeta`, but the gateway parser folds it to `Role::Meta` by its
/// content marker, so this mirrors that parsed role. Shares the group `promptId`.
pub(crate) fn local_command_stdout_line(uuid: &str, prompt_id: &str) -> TranscriptMessage {
    TranscriptMessage {
        role: Role::Meta,
        prompt_id: Some(PromptId::from(prompt_id)),
        ..user_line(
            uuid,
            "<local-command-stdout>\nPENDING review created.\n</local-command-stdout>",
        )
    }
}

/// A harness-injected `<task-notification>` background-task completion, in the
/// real shape: a plain `role: user` line whose `<tool-use-id>` correlates back
/// to the launching tool call.
pub(crate) fn task_notification_line(uuid: &str, tool_use_id: &str) -> TranscriptMessage {
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

/// A `<task-notification>` body that carries only `<task-id>` — the recent
/// Claude Code shape that strips `<tool-use-id>` from the user-message body.
/// Used by the task-id-fallback tests.
pub(crate) fn task_notification_line_task_id_only(uuid: &str, task_id: &str) -> TranscriptMessage {
    user_line(
        uuid,
        &format!(
            "<task-notification>\n\
             <task-id>{task_id}</task-id>\n\
             <status>completed</status>\n\
             <summary>Agent completed</summary>\n\
             </task-notification>"
        ),
    )
}

/// A `<task-notification>` body that carries NEITHER `<tool-use-id>` nor
/// `<task-id>` — the future-Claude-Code shape we want the fold to log a
/// warning for. Used by the tracing-warn test.
pub(crate) fn task_notification_line_both_missing(uuid: &str) -> TranscriptMessage {
    user_line(
        uuid,
        "<task-notification>\n\
         <status>completed</status>\n\
         </task-notification>",
    )
}
