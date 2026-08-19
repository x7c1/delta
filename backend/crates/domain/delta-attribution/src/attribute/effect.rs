//! [`Effect`]: the ordered store/runtime actions the pure fold decides on.

use delta_model::{MessageUuid, ThreadId};

/// A store/runtime action the fold decided on but cannot perform (it is
/// pure). The caller executes these **in order** after the fold returns; the
/// order is exactly the order the previous inline implementation performed
/// them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// A `tool_result` block was ingested: resolve the open permission
    /// request(s) correlated by this `tool_use_id`. `allowed` is inferred
    /// from the result's error flag (a denied tool yields `is_error: true`).
    ResolvePermission { tool_use_id: String, allowed: bool },
    /// The `[Request interrupted by user...]` marker was ingested: the user
    /// aborted the in-flight turn (Claude's `Stop` hook does not fire on
    /// interrupt). Feed `Interrupt` into the turn machine and notify the
    /// browser so the stuck send clears.
    TurnInterrupted,
    /// A synthetic `isApiErrorMessage` assistant line was ingested: the turn
    /// ended on an API error (a usage/session limit, a rate limit, or any other
    /// API failure) rather than completing normally. Like an interrupt, this
    /// turn-end fires **no** `Stop` hook and writes **no** interrupt marker, so
    /// without this effect the turn machine would stay in flight forever and
    /// every later send would defer to `queued` and never dispatch. Feed the
    /// turn machine back to idle and notify the browser so the stuck send
    /// clears. Detected from the structural flag, never the error text, so it
    /// covers every synthetic API-error turn-end and is locale-independent.
    TurnAborted,
    /// A dispatched send was consumed by a client-side slash command rather than
    /// by a model turn, so the turn ended without a `Stop` hook. Two shapes reach
    /// here, both handled by Claude Code entirely client-side (**no**
    /// `UserPromptSubmit` echo, **no** `Stop` hook), yet both were dispatched by
    /// Delta as a send that moved the turn machine to `AwaitingEcho`:
    ///
    /// - a KNOWN local command (e.g. the user ran `/review-pr`): the bare
    ///   command-name line equals the send text inside a recognized
    ///   local-command `promptId` group; and
    /// - an UNKNOWN command (e.g. the user typed `/revew-pr`): Claude rejects it
    ///   with a `type: "system"` / `informational` "Unknown command: …" notice
    ///   and writes no command group at all.
    ///
    /// Without a turn-end signal the dispatched send stays outstanding forever —
    /// wedging the single-outstanding rule so no later send dispatches. This
    /// effect is emitted alongside the [`Effect::SendMatched`] that consumes the
    /// send: feed the turn machine back to idle and notify the browser so the
    /// stuck send clears, exactly like [`Effect::TurnAborted`] does for an
    /// API-error turn-end.
    LocalCommandTurnEnded,
    /// A human user line matched the head outstanding send: mark the send row
    /// matched to this transcript uuid.
    SendMatched {
        send_id: i64,
        matched_uuid: MessageUuid,
    },
    /// A background task was first seen launching: persist
    /// `(tool_use_id -> thread_id)` so its later `<task-notification>` — which
    /// may arrive in a different sync window — can be attributed back to the
    /// launching thread. Two shapes reach here:
    ///
    /// - an async-by-default `Agent`/`Task`, or a Bash with
    ///   `run_in_background: true`, seen as a `tool_use` block on an assistant
    ///   line; and
    /// - a **forked skill**, launched by the CLI harness itself when a slash
    ///   command runs its skill in the background. It writes no `tool_use`
    ///   block at all — only a `<forked-skill-launch>` element on the
    ///   local-command system line — so its `tool_use_id` is synthesized from
    ///   the payload's `agentId` (see
    ///   [`claude_format::ForkedSkillLaunch::tool_use_id`]).
    ///
    /// `task_id` is the background-task identifier when the launch ALREADY
    /// knows it — the forked-skill payload carries its `agentId` up front. It
    /// is `None` for a `tool_use`-driven launch, which learns the id only
    /// later from the launching tool's `tool_result` (via the
    /// `PostToolUse(Agent)` hook, or the fold-time recovery that mirrors it).
    ///
    /// [`claude_format::ForkedSkillLaunch::tool_use_id`]: crate::claude_format::ForkedSkillLaunch::tool_use_id
    SubagentLaunched {
        tool_use_id: String,
        thread_id: ThreadId,
        task_id: Option<String>,
    },
    /// A background task's `<task-notification>` was folded and matched a
    /// recorded launch: clear the persisted `(tool_use_id -> thread_id)`
    /// correlation now that it has been consumed.
    SubagentCompleted { tool_use_id: String },
    /// A subagent launch was seen in the PARENT session's transcript: light up
    /// the running-subagent indicator for it. Two signals produce it — an
    /// `Agent`/`Task` tool_use block, and the `<forked-skill-launch>` element a
    /// slash command's background skill is launched by (which writes no
    /// tool_use at all). A tool_use is emitted regardless of
    /// `run_in_background` — a foreground subagent and a background one both
    /// need the indicator while they run, and they only differ in how the
    /// indicator is cleared (the matching `PostToolUse` for a foreground entry,
    /// the completion `<task-notification>` for a background one); a forked
    /// skill is always background.
    ///
    /// This is the parent-transcript-driven source of truth for the indicator,
    /// replacing the older PreToolUse-driven mechanism. A nested subagent's own
    /// `Agent`/`Task` tool_use is written to the SUBAGENT's JSONL (not the
    /// parent's), so a fold over the parent's transcript naturally excludes
    /// nested launches — and a nested launch never produces a stuck indicator
    /// on the parent.
    SubagentIndicatorStarted {
        tool_use_id: String,
        thread_id: ThreadId,
        subagent_type: Option<String>,
        description: Option<String>,
        background: bool,
    },
    /// A `Role::CompactSummary` line was folded: Claude Code finished
    /// compacting the session (either via auto-`/compact` on resume of a
    /// near-full-context session, or a manual `/compact`). The user's prompt
    /// — if one had been keyed in at the same moment — was swallowed by the
    /// compaction routine and never echoed, so any `Dispatched`
    /// `OutstandingSend` is stuck behind a missing echo. The caller re-types
    /// each such send to the TUI so the user's intent is preserved. The hook
    /// path emits the same signal via `SessionStart(source=compact)`; both
    /// flow through one helper, debounced so a live session that observes
    /// the summary line on the same tick does not fire twice.
    AutoCompactFinished,
}
