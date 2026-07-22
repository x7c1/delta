//! [`attribute_lines`]: the per-line orchestration loop of the attribution fold.

use delta_model::{Message, Role, SessionId, ThreadId};

use crate::claude_format;
use crate::transcript_message::TranscriptMessage;

use super::content_blocks::process_content_blocks;
use super::thread_resolution::resolve_line_thread;
use super::{Attributed, AttributionState, Effect};

/// Attribute a batch of parsed transcript lines to threads.
///
/// Attribution is driven by comparing a user line's trimmed text against the
/// head outstanding (`dispatched`) send — at most one exists under the
/// single-outstanding dispatch rule. Lines are processed in order while
/// maintaining `carry_thread`, the thread of the current turn:
///
/// - A **human** user line (a user line carrying author-written text) that
///   equals the head outstanding send's text is attributed to that send's
///   thread (the new child thread for a branch send), the send is consumed
///   (reported via [`Effect::SendMatched`]), and `carry_thread` advances to
///   it. A human user line matching no outstanding send is external input and
///   lands on `main`, resetting `carry_thread` — unless it is an uncorrelated
///   `queued_command`, a programmatic injection that inherits `carry_thread`.
/// - Every other line follows `carry_thread` — the thread of the turn it
///   belongs to. This covers assistant/system lines AND tool-result lines,
///   which Claude delivers as `role: user` but which are part of the
///   in-flight turn, not a new human turn. The interrupt marker is also a
///   `role: user` line belonging to the aborted turn: it inherits
///   `carry_thread` and additionally yields [`Effect::TurnInterrupted`]. A
///   synthetic `isApiErrorMessage` assistant line (a turn that ended on a
///   usage/session limit, a rate limit, or any other API error) likewise
///   inherits `carry_thread` and additionally yields [`Effect::TurnAborted`],
///   the turn-end signal it carries in place of the absent `Stop` hook /
///   interrupt marker. A `<task-notification>` (a harness-injected
///   background-task completion, delivered as a plain `role: user` line) is a
///   programmatic continuation, so it never resets to `main`. It is attributed
///   to the thread that LAUNCHED the task: its `<tool-use-id>` is looked up in
///   `launched_threads` (recorded when the background `Agent`/`Task`/`Bash`
///   tool_use was first seen), so the completion lands on the launching thread
///   even when the user has moved to a different thread while the task ran.
///   Recent Claude Code versions sometimes drop `<tool-use-id>` from the
///   notification body while keeping `<task-id>`, so the lookup falls back to
///   the `<task-id>` element matched against each entry's persisted `task_id`
///   (learned at `PostToolUse(Agent)` time). Only when neither key matches a
///   recorded launch (the launch fell in an earlier, no-longer-seeded window,
///   or both elements were stripped) does it fall back to inheriting
///   `carry_thread`.
///
/// Whenever an assistant line carries a tool_use that
/// [`claude_format::launches_in_background`] classifies as background — an
/// async-by-default `Agent`/`Task`, or a Bash with `run_in_background: true` —
/// its `id` is recorded against the current `carry_thread` (the launching
/// thread) and emitted as [`Effect::SubagentLaunched`] for the caller to
/// persist; the matching notification later clears it via
/// [`Effect::SubagentCompleted`].
pub fn attribute_lines(
    session_id: &SessionId,
    main_thread: ThreadId,
    mut state: AttributionState,
    lines: Vec<TranscriptMessage>,
) -> Attributed {
    let mut messages = Vec::with_capacity(lines.len());
    let mut effects = Vec::new();

    for line in lines {
        let content_text = Message::flatten_text(&line.content);
        let trimmed_content = content_text.as_deref().unwrap_or("").trim();

        // Slash/local-command grouping. A local command (e.g. `/review-pr`) is
        // recorded as several `type: "user"` lines sharing one `promptId`: a
        // leading `<local-command-caveat>` Claude flags `isMeta` (already
        // `Role::Meta`), the bare command-name line, then the command's
        // `<local-command-stdout>`/`<local-command-stderr>` output (folded to
        // `Role::Meta` by the parser's content check). Record the caveat's
        // `promptId` so the OTHER members are recognized as command machinery.
        if matches!(line.role, Role::Meta)
            && claude_format::is_local_command_caveat(trimmed_content)
        {
            if let Some(prompt_id) = line.prompt_id.clone() {
                state.local_command_prompts.insert(prompt_id);
            }
        }

        // A `type: "user"` line sharing a recognized local-command `promptId`
        // (the bare command-name line — the output lines already arrive as
        // `Role::Meta`) is command machinery, not a human turn. Fold it to
        // `Role::Meta` so it renders collapsed instead of as a user bubble, and
        // — crucially — exclude it from `is_human_turn` so it does not run
        // through external-input handling on `main`.
        //
        // Exception: a `promptSource: "queued"` replay is a genuine human turn
        // even when its `promptId` collides with a local-command group's. Claude
        // Code reuses the current `promptId` for the queued-prompt replay it
        // emits post-compact, so a prompt the user typed *while* an
        // auto- or manual `/compact` was running ends up sharing the compact
        // group's `promptId`. Excluding queued replays here keeps the replay
        // out of the group's Meta reclassification, so it flows through the
        // normal `is_human_turn` branch (matches the head outstanding send by
        // text, emits `SendMatched`, and attributes to the send's thread).
        let in_local_command_group = !line.is_queued_replay
            && line
                .prompt_id
                .as_ref()
                .is_some_and(|id| state.local_command_prompts.contains(id));
        let role = if in_local_command_group && matches!(line.role, Role::User) {
            Role::Meta
        } else {
            line.role
        };
        let is_local_command_name_line = in_local_command_group
            && matches!(line.role, Role::User)
            && !trimmed_content.is_empty();

        process_content_blocks(&mut state, &mut effects, &line.content);

        // A genuine human turn is a user line with author-written text.
        // Claude delivers tool results as `role: user` lines too, but those
        // belong to the in-flight turn, not a new human turn, so they must
        // inherit `carry_thread` rather than reset it to `main`. (Mirrors the
        // frontend's `isUserTurn`.) Treating a tool_result as a turn boundary
        // used to drop the rest of a sub-thread's turn onto `main`.
        //
        // An interrupt marker is also a `role: user` line, but it belongs to
        // the turn the user just aborted, not a new human turn — so it too
        // inherits `carry_thread` and is excluded from `is_human_turn` (it
        // must not run through send correlation nor reset to `main`).
        //
        // A `<task-notification>` is a third such `role: user` line: the
        // harness injects it to report a background task's completion, so it is
        // a programmatic continuation of the in-flight turn, not a new human
        // turn. Claude delivers it as a normal `type: "user"` line (NOT a
        // legacy `queued_command` attachment), so the parser does not flag it
        // `is_queued_command`. It must likewise be excluded from
        // `is_human_turn` and inherit `carry_thread`; otherwise, when a
        // background task completes while the user is working in a sub-thread,
        // the notification (and the assistant's continuation, and every later
        // turn) would reset to `main`.
        // Classify against the reclassified `role`: a local-command member that
        // was folded to `Role::Meta` above is no longer a human turn (the
        // command-name line is handled by its own branch below).
        let trimmed = trimmed_content;
        let is_interrupt_marker =
            matches!(role, Role::User) && claude_format::is_interrupt_marker(trimmed);
        let is_task_notification =
            matches!(role, Role::User) && claude_format::is_task_notification(trimmed);
        // The unknown-command notice Claude Code writes for an unrecognized slash
        // command. The parser surfaces it as a `Role::System` line carrying
        // `Unknown command: <command>` (see the gateway's informational-subtype
        // handling). Like a known local command it fires no echo and no `Stop`.
        let is_unknown_command_notice =
            matches!(role, Role::System) && claude_format::is_unknown_command_notice(trimmed);
        let is_human_turn = matches!(role, Role::User)
            && !trimmed.is_empty()
            && !is_interrupt_marker
            && !is_task_notification;

        if is_interrupt_marker {
            effects.push(Effect::TurnInterrupted);
        }

        // A synthetic `isApiErrorMessage` assistant line ends the turn on an API
        // error without a `Stop` hook or an interrupt marker. Emit a turn-end
        // effect so the caller feeds the turn machine back to idle; the line is
        // otherwise ingested and attributed like any assistant line (it inherits
        // `carry_thread` via the non-human-turn branch below, so this does not
        // change thread attribution).
        if line.is_api_error {
            effects.push(Effect::TurnAborted);
        }

        // A `Role::CompactSummary` line marks the end of a Claude Code
        // compaction group (auto-`/compact` on resume of a near-full-context
        // session, or a manual `/compact`). The compaction routine swallows any
        // prompt the user keyed in at the same moment, so a `Dispatched`
        // `OutstandingSend` is stuck behind a missing echo: emit
        // `AutoCompactFinished` so the caller re-types it. The line itself
        // keeps its existing role-based handling (inherits `carry_thread`,
        // emits no `SendMatched`) — see the regression covered by
        // `a_compact_summary_line_inherits_carry_and_does_not_consume_the_outstanding_send`.
        if matches!(role, Role::CompactSummary) {
            effects.push(Effect::AutoCompactFinished);
        }

        let (thread_id, semantic_parent_uuid) = resolve_line_thread(
            session_id,
            main_thread,
            &mut state,
            &mut effects,
            &line.uuid,
            trimmed,
            line.is_queued_command,
            is_local_command_name_line,
            is_unknown_command_notice,
            is_human_turn,
            is_task_notification,
        );

        messages.push(Message {
            uuid: line.uuid,
            // Claude reconstructs messages from JSONL lines, not provider items,
            // and its streaming preview is dropped-and-replaced (no id join), so
            // it never carries a provider item id.
            provider_item_id: None,
            session_id: session_id.clone(),
            thread_id,
            // The reclassified role: a local-command command-name line folds to
            // `Role::Meta` so it renders collapsed, not as a user bubble.
            role,
            linear_parent_uuid: line.linear_parent_uuid,
            semantic_parent_uuid,
            prompt_id: line.prompt_id,
            // Persist the message's own transcript line index as its `seq`,
            // so ordering follows true file position with no drift.
            seq: line.seq,
            content_text,
            content: line.content,
            created_at: line.created_at,
            // Transcript-derived per-message metadata, carried straight through.
            model: line.model,
            git_branch: line.git_branch,
            cwd: line.cwd,
            response_time_ms: line.response_time_ms,
        });
    }

    Attributed {
        messages,
        effects,
        state,
    }
}
