//! Shared helpers for the attribution test binaries.
//!
//! Each test target under `tests/` is its own crate and compiles its own copy
//! of this module, using a different subset of it — hence the dead-code allow.
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
        is_queued_replay: false,
        is_api_error: false,
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
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
/// note in docs/guides/development/canary.md).
pub fn queued_command_line(uuid: &str, text: &str) -> TranscriptMessage {
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
/// exclusion in attribution.
pub fn queued_replay_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        is_queued_replay: true,
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

/// An assistant line launching an `Agent`/`Task` tool call in the modern shape:
/// no `run_in_background` key. Modern Claude Code dropped that parameter from
/// the schema and made these calls async by default, so the predicate
/// classifies them as background — matching what `background_tool_use_line`
/// gets via the explicit flag.
pub fn modern_agent_tool_use_line(
    uuid: &str,
    tool_use_id: &str,
    tool_name: &str,
) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolUse {
            id: tool_use_id.into(),
            name: tool_name.into(),
            input: serde_json::json!({
                "subagent_type": "general-purpose",
                "description": "Run ls and count entries",
                "prompt": "…",
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

/// A `tool_result` carrier whose `content` carries the launch ack text that a
/// background `Agent`/`Task` writes — including the `agentId: <id>` substring
/// the fold-time recovery is supposed to capture. Mirrors the real Claude
/// shape (array of `{ "type": "text", "text": ... }` blocks).
pub fn tool_result_with_agent_id_line(
    uuid: &str,
    tool_use_id: &str,
    agent_id: &str,
) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: serde_json::json!([{
                "type": "text",
                "text": format!(
                    "Async agent launched successfully.\n\
                     agentId: {agent_id} (internal ID - do not mention to user.)\n\
                     The agent is working in the background."
                ),
            }]),
            is_error: false,
        }],
        ..user_line(uuid, "")
    }
}

/// A `<task-notification>` whose body carries only `<task-id>` — no
/// `<tool-use-id>`. Recent Claude Code versions strip the element from the
/// user-message body while keeping the sibling `<task-id>` element, so the
/// completion must still correlate via the task-id fallback.
pub fn task_notification_line_with_task_id_only(uuid: &str, task_id: &str) -> TranscriptMessage {
    user_line(
        uuid,
        &format!(
            "<task-notification>\n\
             <task-id>{task_id}</task-id>\n\
             <output-file>/tmp/x.output</output-file>\n\
             <status>completed</status>\n\
             <summary>Agent completed</summary>\n\
             </task-notification>"
        ),
    )
}

/// A `<task-notification>` whose body carries only `<tool-use-id>` — no
/// `<task-id>`. Used to regression-test the existing tool-use-id-keyed
/// correlation path after the fold-time `task_id` upgrade.
pub fn task_notification_line_with_tool_use_id_only(
    uuid: &str,
    tool_use_id: &str,
) -> TranscriptMessage {
    user_line(
        uuid,
        &format!(
            "<task-notification>\n\
             <tool-use-id>{tool_use_id}</tool-use-id>\n\
             <output-file>/tmp/x.output</output-file>\n\
             <status>completed</status>\n\
             <summary>Agent completed</summary>\n\
             </task-notification>"
        ),
    )
}

/// An assistant line RETRIEVING a background task's result: a `TaskOutput`
/// `tool_use` naming the task and blocking until it finishes. The parent reads
/// the result itself this way, and the harness then injects NO
/// `<task-notification>` — the retrieval's own result is the completion signal.
pub fn task_output_tool_use_line(
    uuid: &str,
    tool_use_id: &str,
    task_id: &str,
) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolUse {
            id: tool_use_id.into(),
            name: "TaskOutput".into(),
            input: serde_json::json!({ "task_id": task_id, "block": true }),
        }],
        ..assistant_line(uuid, "")
    }
}

/// The `tool_result` carrier of a `TaskOutput` retrieval, in the real body
/// shape: a PLAIN STRING content (not the array of text blocks a
/// model-authored result uses) carrying `<retrieval_status>` (did the
/// retrieval work), then the retrieved task's `<task_id>`, `<task_type>` and
/// `<status>` (`completed` / `failed` / `killed`, or `running` for a
/// non-blocking poll of a task still working).
pub fn task_output_result_line(
    uuid: &str,
    tool_use_id: &str,
    task_id: &str,
    status: &str,
    is_error: bool,
) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: serde_json::json!(format!(
                "<retrieval_status>success</retrieval_status>\n\n\
                 <task_id>{task_id}</task_id>\n\n\
                 <task_type>local_agent</task_type>\n\n\
                 <status>{status}</status>\n\n\
                 <output>\nthe agent's report\n</output>"
            )),
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

/// The synthetic user line Claude Code writes when `/compact` runs, carrying
/// the previous-conversation summary. Flagged `isCompactSummary`, so the
/// parser classifies it `Role::CompactSummary`; attribution must skip it
/// (no send match, no `carry_thread` reset).
pub fn compact_summary_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        role: Role::CompactSummary,
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

/// The `type: "system"` / `subtype: "local_command"` line Claude Code writes
/// when a slash command forks its skill into a BACKGROUND agent (e.g.
/// `/review-pr`, recorded as `/example:review-pr`). The gateway parser
/// folds the subtype to `Role::Meta` and surfaces the top-level `content`, so
/// this mirrors the parsed shape: the command's `<local-command-stdout>` plus
/// the `<forked-skill-launch>` element carrying the launch payload.
///
/// It deliberately carries NO `promptId` — the real line does not — so it is
/// not a member of the local-command `promptId` group, and the fold must
/// recognize it by content alone.
pub fn forked_skill_launch_line(uuid: &str, agent_id: &str, skill_name: &str) -> TranscriptMessage {
    meta_line(
        uuid,
        &format!(
            "<local-command-stdout>Running in the background as @{skill_name}\
             </local-command-stdout>\n\
             <forked-skill-launch>{{\"agentId\":\"{agent_id}\",\
             \"skillName\":\"{skill_name}\",\
             \"description\":\"/{skill_name}\"}}</forked-skill-launch>"
        ),
    )
}

/// A `<forked-skill-launch>` line whose element body Delta cannot use: the
/// caller supplies the raw body (malformed JSON, or JSON naming no `agentId`).
/// Without the correlation key nothing can be tracked, so the fold must emit
/// no effects — and log, so a Claude Code format change surfaces there rather
/// than as a silently dark indicator.
pub fn forked_skill_launch_line_with_body(uuid: &str, body: &str) -> TranscriptMessage {
    meta_line(
        uuid,
        &format!(
            "<local-command-stdout>Running in the background</local-command-stdout>\n\
             <forked-skill-launch>{body}</forked-skill-launch>"
        ),
    )
}

/// The unknown-command notice Claude Code writes when the user types a slash
/// command it does not recognize (e.g. `/review-pr` when no such command
/// exists): a `type: "system"` / `subtype: "informational"` line whose top-level
/// content the gateway parser surfaces as `Unknown command: <command>`, so the
/// parsed role is `Role::System`. Like a known local command it fires no echo and
/// no `Stop`, so it must unstick the send Delta dispatched for the command.
pub fn unknown_command_notice_line(uuid: &str, command: &str) -> TranscriptMessage {
    TranscriptMessage {
        role: Role::System,
        ..user_line(uuid, &format!("Unknown command: {command}"))
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
        task_id: None,
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
