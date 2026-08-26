//! Attribution-fold semantics, tested directly on the pure function.
//!
//! These pin the line-classification and thread-assignment rules without any
//! store or actor: ports of the logic-heavy parts of the `delta-usecase`
//! sync test suite, plus the seams that suite could not reach (effect order,
//! send non-consumption, state threading).

mod support;

use delta_attribution::{attribute_lines, AttributionState, Effect, SubagentLaunch};
use delta_model::MessageUuid;
use support::*;

#[test]
fn a_plain_send_echo_lands_on_its_thread_and_marks_the_send_matched() {
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "hello world"))),
        vec![
            user_line("u-1", "hello world"),
            assistant_line("a-1", "hi there"),
        ],
    );

    assert_eq!(message(&outcome, "u-1").thread_id, MAIN);
    assert_eq!(message(&outcome, "u-1").semantic_parent_uuid, None);
    assert_eq!(message(&outcome, "a-1").thread_id, MAIN);
    assert_eq!(
        outcome.effects,
        vec![Effect::SendMatched {
            send_id: 7,
            matched_uuid: MessageUuid::from("u-1"),
            attributed: true,
        }]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the match consumed the send"
    );
}

#[test]
fn a_branch_send_echo_attributes_user_and_assistant_to_the_child() {
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(
            MAIN,
            Some(branch_send(7, CHILD, "uuid-parent", "branch text")),
        ),
        vec![
            user_line("u-b", "branch text"),
            assistant_line("a-b", "branch reply"),
        ],
    );

    // The echo opens the child thread and carries the branch semantic parent.
    assert_eq!(message(&outcome, "u-b").thread_id, CHILD);
    assert_eq!(
        message(&outcome, "u-b").semantic_parent_uuid,
        Some(MessageUuid::from("uuid-parent"))
    );
    // The assistant reply carries forward to the child, not main.
    assert_eq!(message(&outcome, "a-b").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
}

#[test]
fn a_queued_command_matching_a_send_attributes_prompt_and_reply_to_the_child() {
    // LEGACY FORMAT (older claude versions; see the queued-prompt drift note
    // in docs/guides/development/canary.md): a branch send issued while a turn was
    // in flight was queued by Claude and recorded only as a `queued_command`
    // attachment — never a normal user line. It must still correlate to its
    // send so the prompt AND the reply land on the child thread, not `main`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(
            MAIN,
            Some(branch_send(7, CHILD, "uuid-parent", "branch text")),
        ),
        vec![
            queued_command_line("u-b", "branch text"),
            assistant_line("a-b", "branch reply"),
        ],
    );

    assert_eq!(message(&outcome, "u-b").thread_id, CHILD);
    assert_eq!(
        message(&outcome, "u-b").semantic_parent_uuid,
        Some(MessageUuid::from("uuid-parent"))
    );
    assert_eq!(message(&outcome, "a-b").thread_id, CHILD);
}

#[test]
fn an_unmatched_queued_command_inherits_the_carry_thread() {
    // LEGACY FORMAT (older claude versions; see the queued-prompt drift note
    // in docs/guides/development/canary.md): a queued command that matches no send —
    // e.g. a background task notification injected mid-turn — is a
    // programmatic injection, not stray pane typing, so it must inherit the
    // active thread rather than reset attribution to `main`. (Ported from the
    // actor-level sync test
    // `unmatched_queued_command_mid_branch_stays_on_branch`.)
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            queued_command_line("u-note", "<task-notification>done</task-notification>"),
            assistant_line("a-after", "after the note"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert!(outcome.effects.is_empty());
}

#[test]
fn a_tool_result_mid_branch_stays_on_the_branch_and_resolves_its_permission() {
    // Claude writes the `tool_result` as a `role: user` line; treating it as
    // a new human turn used to drop the rest of the turn onto `main`. (Ported
    // from the actor-level sync test
    // `tool_result_mid_branch_turn_stays_on_the_branch`.)
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            tool_use_line("a-call", "t1"),
            tool_result_line("u-res", "t1", false),
            assistant_line("a-final", "after the tool"),
        ],
    );

    assert_eq!(
        threads(&outcome),
        vec![
            ("a-call".into(), CHILD),
            ("u-res".into(), CHILD),
            ("a-final".into(), CHILD),
        ]
    );
    assert_eq!(
        outcome.effects,
        vec![Effect::ResolvePermission {
            tool_use_id: "t1".into(),
            allowed: true,
        }]
    );
}

#[test]
fn a_denied_tool_result_resolves_its_permission_as_denied() {
    // A denied tool yields `is_error: true` ("User rejected tool use"); the
    // error flag infers allowed vs denied.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![tool_result_line("u-res", "t9", true)],
    );

    assert_eq!(
        outcome.effects,
        vec![Effect::ResolvePermission {
            tool_use_id: "t9".into(),
            allowed: false,
        }]
    );
}

#[test]
fn the_interrupt_marker_inherits_carry_and_does_not_consume_the_outstanding_send() {
    // The marker is a `role: user` line, but it belongs to the turn the user
    // just aborted: it must inherit `carry_thread` (not reset to `main`), must
    // not run through send correlation, and must surface as a TurnInterrupted
    // effect (Claude's `Stop` hook does not fire on interrupt).
    let unrelated = send(9, MAIN, "unrelated prompt");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(unrelated.clone())),
        vec![interrupt_line("u-int")],
    );

    assert_eq!(message(&outcome, "u-int").thread_id, CHILD);
    assert_eq!(outcome.effects, vec![Effect::TurnInterrupted]);
    assert_eq!(
        outcome.state.outstanding,
        vec![unrelated]
            .into_iter()
            .collect::<std::collections::VecDeque<_>>(),
        "the marker must not match or consume the pending send"
    );
}

#[test]
fn both_interrupt_marker_variants_are_recognized() {
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![user_line(
            "u-int",
            "[Request interrupted by user for tool use]",
        )],
    );

    assert_eq!(message(&outcome, "u-int").thread_id, CHILD);
    assert_eq!(outcome.effects, vec![Effect::TurnInterrupted]);
}

#[test]
fn an_api_error_line_inherits_carry_and_emits_turn_aborted_without_consuming_the_send() {
    // A synthetic `isApiErrorMessage` assistant line ends the turn on an API
    // error (a usage/session limit, a rate limit, ...). It is attributed like
    // any assistant line — inheriting `carry_thread`, never resetting to `main`
    // and never running through send correlation — and additionally surfaces a
    // `TurnAborted` effect, the turn-end signal that line carries in place of
    // the absent `Stop` hook / interrupt marker.
    let unrelated = send(9, MAIN, "unrelated prompt");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(unrelated.clone())),
        vec![api_error_line("e-1")],
    );

    assert_eq!(message(&outcome, "e-1").thread_id, CHILD);
    assert_eq!(outcome.effects, vec![Effect::TurnAborted]);
    assert_eq!(
        outcome.state.outstanding,
        vec![unrelated]
            .into_iter()
            .collect::<std::collections::VecDeque<_>>(),
        "the api-error line must not match or consume the pending send"
    );
}

#[test]
fn a_compact_summary_line_inherits_carry_and_does_not_consume_the_outstanding_send() {
    // A `Role::CompactSummary` line is not a human turn: it must inherit
    // `carry_thread` (never reset to `main`) and must not match against an
    // outstanding `dispatched` send by text. The tail `assistant_line` pins
    // downstream propagation — the symptom of a missed inherit is that the
    // next message drifts to `main`.
    //
    // It DOES emit `Effect::AutoCompactFinished` so the caller can re-type
    // any send stuck behind the compaction (a `Dispatched` send whose echo
    // was swallowed by the compaction routine).
    let pending = send(9, MAIN, "the user's actual prompt");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(pending.clone())),
        vec![
            compact_summary_line("cs-1", "<summary>of the previous conversation</summary>"),
            assistant_line("a-after", "resuming the sub-thread work"),
        ],
    );

    assert_eq!(message(&outcome, "cs-1").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert_eq!(
        outcome.effects,
        vec![Effect::AutoCompactFinished],
        "the compact-summary line emits exactly one AutoCompactFinished and \
         no SendMatched (it must not consume the pending send by text)"
    );
    assert_eq!(
        outcome.state.outstanding,
        vec![pending]
            .into_iter()
            .collect::<std::collections::VecDeque<_>>(),
        "the compact-summary line must not match or consume the pending send"
    );
}

#[test]
fn auto_compact_finished_is_not_emitted_for_non_compact_summary_lines() {
    // Plain user / meta / assistant / other lines must not emit
    // `Effect::AutoCompactFinished` — only `Role::CompactSummary` is the
    // signal. Asserted on each non-compact-summary line individually so a
    // regression that fires the effect spuriously on any of them is caught.
    for line in [
        user_line("u-plain", "hello"),
        meta_line("m-1", "<system-reminder>noop</system-reminder>"),
        assistant_line("a-1", "ok"),
        other_line("o-1"),
    ] {
        let outcome = attribute_lines(
            &session(),
            MAIN,
            AttributionState::new(MAIN, None),
            vec![line],
        );
        assert!(
            !outcome
                .effects
                .iter()
                .any(|e| matches!(e, Effect::AutoCompactFinished)),
            "AutoCompactFinished must not fire for a non-compact-summary line: \
             got effects {:?}",
            outcome.effects
        );
    }
}

#[test]
fn a_task_notification_mid_branch_inherits_carry_and_does_not_reset_to_main() {
    // A `<task-notification>` is a harness-injected background-task completion,
    // delivered as a plain `role: user` line (NOT a legacy `queued_command`
    // attachment). It is a programmatic continuation of the in-flight turn, not
    // a new human turn, so it must inherit `carry_thread` — the notification,
    // the assistant's continuation, and every later turn must stay on the
    // sub-thread rather than dropping onto `main`. It must also not run through
    // send correlation. (Regression: keying the inherit guard on
    // `is_queued_command`, which real task-notifications do not carry, let them
    // fall through to the `main` reset.)
    let unrelated = send(9, MAIN, "unrelated prompt");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(unrelated.clone())),
        vec![
            task_notification_line("u-note"),
            assistant_line("a-after", "resuming the sub-thread work"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert!(
        outcome.effects.is_empty(),
        "a task-notification neither resolves a permission nor matches a send"
    );
    assert_eq!(
        outcome.state.outstanding,
        vec![unrelated]
            .into_iter()
            .collect::<std::collections::VecDeque<_>>(),
        "the notification must not match or consume the pending send"
    );
}

#[test]
fn a_background_task_completion_is_attributed_to_its_launching_thread() {
    // A background subagent is launched while the in-flight turn is on the
    // child thread (carry = CHILD). The launch records `(toolu-bg -> CHILD)`.
    // The user then moves to and works on `main` (carry resets to MAIN via an
    // external line). When the `<task-notification>` for `toolu-bg` finally
    // lands, it — and the assistant continuation it drives — must be attributed
    // to CHILD (the launching thread), NOT to MAIN (the current thread). Before
    // the correlation fix the notification blindly inherited `carry_thread` and
    // landed on MAIN.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            // The user leaves the sub-thread: an external human turn on main.
            user_line("u-ext", "now working on something else"),
            assistant_line("a-ext", "sure, on it"),
            // The background task completes; its notification correlates back.
            task_notification_line_for("u-note", "toolu-bg"),
            assistant_line("a-after", "the background agent finished"),
        ],
    );

    assert_eq!(message(&outcome, "a-launch").thread_id, CHILD);
    assert_eq!(message(&outcome, "u-ext").thread_id, MAIN);
    assert_eq!(message(&outcome, "a-ext").thread_id, MAIN);
    // The notification lands on the LAUNCHING thread, not the current one.
    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    // ...and the assistant's continuation of it follows onto that thread.
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);

    // The launch is recorded, the parent-transcript indicator effect fires for
    // the `Agent` tool_use, then the completion clears the launch.
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "toolu-bg".into(),
                thread_id: CHILD,
                task_id: None,
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "toolu-bg".into(),
                thread_id: CHILD,
                subagent_type: Some("general-purpose".into()),
                description: None,
                background: true,
            },
            Effect::SubagentCompleted {
                tool_use_id: "toolu-bg".into(),
            },
        ]
    );
    // The completion drained the correlation from the carried state.
    assert!(outcome.state.launched_threads.is_empty());
}

#[test]
fn a_denied_background_launch_completes_from_its_errored_tool_result() {
    // A background `Agent`/`Task` launch that the permission/auto-mode
    // classifier DENIES still writes its `tool_use` block to the parent JSONL,
    // so the running indicator lights. But the launch never happened, so no
    // `<task-notification>` will ever arrive to complete it, and the turn-end
    // sweep keeps background entries — the indicator would stay stuck forever.
    // The denial surfaces as an `is_error: true` `tool_result` for the same
    // `tool_use_id`; folding it must emit `SubagentCompleted` (alongside the
    // usual `ResolvePermission { allowed: false }`) and drop the launch entry.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            tool_result_line("u-result", "toolu-bg", true),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                task_id: None,
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                subagent_type: Some("general-purpose".into()),
                description: None,
                background: true,
            },
            Effect::ResolvePermission {
                tool_use_id: "toolu-bg".into(),
                allowed: false,
            },
            Effect::SubagentCompleted {
                tool_use_id: "toolu-bg".into(),
            },
        ],
        "a denied background launch must complete from its errored tool_result"
    );
    // The entry is drained so a later stray notification can't double-fire.
    assert!(outcome.state.launched_threads.is_empty());
}

#[test]
fn a_successful_background_launch_result_does_not_complete_the_subagent() {
    // The negative: a background launch whose `tool_result` is NOT an error
    // (`is_error: false`) is a normal async launch. It must NOT be completed by
    // the result — only its later `<task-notification>` completes it. The
    // errored-launch clear must fire strictly on `is_error: true`, so this
    // yields no `SubagentCompleted` and the launch entry survives, waiting for
    // its notification.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            tool_result_line("u-result", "toolu-bg", false),
        ],
    );

    assert!(
        !outcome
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SubagentCompleted { .. })),
        "a non-errored launch result must not complete the subagent, got {:?}",
        outcome.effects
    );
    // The launch survives for its `<task-notification>` to consume later.
    assert!(outcome.state.launched_threads.contains_key("toolu-bg"));
}

#[test]
fn an_errored_result_for_a_non_launch_tool_emits_no_completion() {
    // The gate is precise: an `is_error: true` `tool_result` for a tool that
    // was NOT recorded as a background launch (e.g. an ordinary failed tool
    // never seeded into `launched_threads`) must resolve its permission but
    // must NOT emit a spurious `SubagentCompleted`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            tool_use_line("a-tool", "toolu-plain"),
            tool_result_line("u-result", "toolu-plain", true),
        ],
    );

    assert!(
        !outcome
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SubagentCompleted { .. })),
        "an errored non-launch tool must not emit SubagentCompleted, got {:?}",
        outcome.effects
    );
    assert!(outcome
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ResolvePermission { allowed: false, .. })));
}

#[test]
fn a_background_completion_seeded_from_an_earlier_window_lands_on_its_launch() {
    // The launch fell in an earlier sync window; only the persisted
    // `(toolu-bg -> CHILD)` map is reseeded (no launch line in this batch).
    // Carry is MAIN (the user has since moved on). The notification must still
    // resolve to CHILD via the seeded map, and only `SubagentCompleted` fires.
    let mut launched = std::collections::BTreeMap::new();
    launched.insert(
        "toolu-bg".to_owned(),
        SubagentLaunch {
            thread_id: CHILD,
            task_id: None,
        },
    );
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::with_launches(MAIN, None, launched),
        vec![task_notification_line_for("u-note", "toolu-bg")],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(
        outcome.effects,
        vec![Effect::SubagentCompleted {
            tool_use_id: "toolu-bg".into(),
        }]
    );
    assert!(outcome.state.launched_threads.is_empty());
}

#[test]
fn a_modern_agent_tool_use_with_no_run_in_background_flag_is_treated_as_background() {
    // Modern Claude Code (>= v2.1.193) dropped the `run_in_background` parameter
    // from the `Agent`/`Task` tool schema and made these calls async by default.
    // An assistant line carrying such a tool_use — with no `run_in_background`
    // key at all — must still emit both `SubagentLaunched` (so a later
    // `<task-notification>` can correlate back to the launching thread) and
    // `SubagentIndicatorStarted { background: true }` (so the running indicator
    // SURVIVES the immediate `PostToolUse(Agent)` that fires when the launch
    // returned).
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![modern_agent_tool_use_line(
            "a-launch",
            "toolu-modern",
            "Agent",
        )],
    );

    assert_eq!(message(&outcome, "a-launch").thread_id, CHILD);
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "toolu-modern".into(),
                thread_id: CHILD,
                task_id: None,
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "toolu-modern".into(),
                thread_id: CHILD,
                subagent_type: Some("general-purpose".into()),
                description: Some("Run ls and count entries".into()),
                background: true,
            },
        ],
        "modern Agent shape (no flag) must launch as background and light a background indicator"
    );
    // The launch is recorded for a later `<task-notification>` to consume.
    assert!(outcome.state.launched_threads.contains_key("toolu-modern"));
}

#[test]
fn a_task_notification_carrying_only_task_id_resolves_via_the_tool_result_upgrade() {
    // Recent Claude Code versions ship `<task-notification>` bodies with only
    // `<task-id>` (no `<tool-use-id>`). Without the fold-time upgrade the
    // task-id fallback finds no match — every launch entry's `task_id` is
    // still `None` because the live `PostToolUse(Agent)` hook did not run
    // during a cold-start replay — and the running indicator stays lit. The
    // tool_result of the launching tool_use carries `agentId: <id>` in its
    // human-readable text; the fold-time recovery captures it into the launch
    // entry's `task_id`, so the later notification correlates by task-id.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            tool_result_with_agent_id_line("u-result", "toolu-bg", "agent-xyz"),
            task_notification_line_with_task_id_only("u-note", "agent-xyz"),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                task_id: None,
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                subagent_type: Some("general-purpose".into()),
                description: None,
                background: true,
            },
            Effect::ResolvePermission {
                tool_use_id: "toolu-bg".into(),
                allowed: true,
            },
            Effect::SubagentCompleted {
                tool_use_id: "toolu-bg".into(),
            },
        ]
    );
    assert!(outcome.state.launched_threads.is_empty());
}

#[test]
fn a_task_notification_carrying_only_tool_use_id_still_completes() {
    // Regression: when the notification body still carries `<tool-use-id>`
    // (the long-standing shape), the tool-use-id-keyed lookup must continue
    // to match. The fold-time upgrade runs harmlessly — it writes the
    // `task_id` onto the entry — but the completion resolves on the
    // tool-use-id path, exactly as before.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            tool_result_with_agent_id_line("u-result", "toolu-bg", "agent-xyz"),
            task_notification_line_with_tool_use_id_only("u-note", "toolu-bg"),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                task_id: None,
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                subagent_type: Some("general-purpose".into()),
                description: None,
                background: true,
            },
            Effect::ResolvePermission {
                tool_use_id: "toolu-bg".into(),
                allowed: true,
            },
            Effect::SubagentCompleted {
                tool_use_id: "toolu-bg".into(),
            },
        ]
    );
    assert!(outcome.state.launched_threads.is_empty());
}

#[test]
fn an_unknown_background_completion_falls_back_to_inheriting_carry() {
    // A `<task-notification>` whose `<tool-use-id>` is not in the launch map
    // (its launch fell in a window no longer seeded) must not regress: it
    // inherits `carry_thread` exactly as before, emits no completion effect,
    // and does not reset to `main`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            task_notification_line_for("u-note", "toolu-unknown"),
            assistant_line("a-after", "resuming"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert!(
        outcome.effects.is_empty(),
        "an uncorrelated task-notification emits no completion effect"
    );
}

#[test]
fn a_human_line_whose_text_differs_still_consumes_the_outstanding_send() {
    // Consumption is POSITIONAL: while a send is outstanding its keystrokes are
    // already in the pane, so the next human user line is that send's echo
    // whatever text Claude Code recorded (a prompt rewrite, or characters typed
    // on top of the pasted text). It lands on the send's thread — together with
    // the reply that follows it — and the send is consumed. The text mismatch
    // survives only as `attributed: false`, the log-worthy hint that the echo
    // was not verbatim.
    let pending = branch_send(7, CHILD, "uuid-parent", "branch text");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(pending)),
        vec![
            user_line("u-b", "branch text with extra characters"),
            assistant_line("a-b", "branch reply"),
        ],
    );

    assert_eq!(message(&outcome, "u-b").thread_id, CHILD);
    assert_eq!(
        message(&outcome, "u-b").semantic_parent_uuid,
        Some(MessageUuid::from("uuid-parent"))
    );
    // The reply follows the echo onto the branch instead of vanishing on main.
    assert_eq!(message(&outcome, "a-b").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert_eq!(
        outcome.effects,
        vec![Effect::SendMatched {
            send_id: 7,
            matched_uuid: MessageUuid::from("u-b"),
            attributed: false,
        }]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the send is consumed by position, so it never lingers dispatched"
    );
}

#[test]
fn a_human_line_with_no_outstanding_send_resets_carry_to_main() {
    // The other half of the positional rule: with nothing outstanding, no send
    // can explain this line, so it is input typed straight into the pane. It
    // opens a turn on `main` and resets `carry_thread` — the reply follows it
    // to main — and no send effect is emitted.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            user_line("u-ext", "typed straight into the pane"),
            assistant_line("a-ext", "external reply"),
        ],
    );

    assert_eq!(message(&outcome, "u-ext").thread_id, MAIN);
    assert_eq!(message(&outcome, "a-ext").thread_id, MAIN);
    assert_eq!(outcome.state.carry_thread, MAIN);
    assert!(
        outcome.effects.is_empty(),
        "there was no send to consume, so nothing is reported as matched"
    );
}

#[test]
fn non_human_lines_inherit_carry() {
    // Meta (harness-injected), unclassified, and empty-text user lines are
    // not human turns: none of them resets or advances `carry_thread`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            meta_line("m-1", "<system-reminder>injected</system-reminder>"),
            other_line("o-1"),
            user_line("u-blank", "   "),
        ],
    );

    assert_eq!(
        threads(&outcome),
        vec![
            ("m-1".into(), CHILD),
            ("o-1".into(), CHILD),
            ("u-blank".into(), CHILD),
        ]
    );
    assert_eq!(outcome.state.carry_thread, CHILD);
}

#[test]
fn send_text_comparison_ignores_surrounding_whitespace() {
    // The text comparison no longer decides consumption (position does), but it
    // still decides the `attributed` flag — and it compares TRIMMED text, so a
    // send whose stored text carries surrounding whitespace is still recognized
    // as echoed verbatim and raises no rewrite warning.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, CHILD, "  spaced prompt \n"))),
        vec![user_line("u-1", "spaced prompt")],
    );

    assert_eq!(message(&outcome, "u-1").thread_id, CHILD);
    assert_eq!(
        outcome.effects,
        vec![Effect::SendMatched {
            send_id: 7,
            matched_uuid: MessageUuid::from("u-1"),
            attributed: true,
        }]
    );
}

#[test]
fn state_threads_across_batches_exactly_like_one_fold() {
    // The returned state is the exact seed for the lines that follow: folding
    // in two batches equals folding everything at once. (The exhaustive
    // version of this property runs over the golden corpus.)
    let lines = vec![
        user_line("u-b", "branch text"),
        assistant_line("a-b", "branch reply"),
        interrupt_line("u-int"),
        user_line("u-ext", "external"),
    ];
    let seed = AttributionState::new(MAIN, Some(branch_send(7, CHILD, "p", "branch text")));

    let whole = attribute_lines(&session(), MAIN, seed.clone(), lines.clone());

    let first = attribute_lines(&session(), MAIN, seed, lines[..2].to_vec());
    let second = attribute_lines(&session(), MAIN, first.state.clone(), lines[2..].to_vec());

    let mut stitched_messages = first.messages.clone();
    stitched_messages.extend(second.messages.clone());
    let mut stitched_effects = first.effects.clone();
    stitched_effects.extend(second.effects.clone());

    assert_eq!(whole.messages, stitched_messages);
    assert_eq!(whole.effects, stitched_effects);
    assert_eq!(whole.state, second.state);
}

// --- Slash/local commands ------------------------------------------------------

#[test]
fn a_local_command_group_folds_to_meta_resolves_its_send_and_ends_the_turn() {
    // The user ran `/review-pr` as the session's first prompt. Delta dispatched
    // it as send 7, so the turn machine is AwaitingEcho{7}. Claude handles the
    // local command client-side: a 3-line group sharing one promptId (the
    // isMeta caveat, the bare command-name line, the stdout), and NO
    // UserPromptSubmit echo / NO Stop. The command-name and stdout lines must
    // fold to `Meta` (not render as user bubbles), send 7 must be consumed
    // against the command-name line, and the degenerate turn must end so the
    // send is freed and the machine can return to idle.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    // Every member of the group folds to meta (so the conversation pane
    // collapses them rather than showing user bubbles).
    assert_eq!(message(&outcome, "caveat").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "cmdname").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "stdout").role, delta_model::Role::Meta);

    // The command-name line consumes the dispatched send and ends the turn, in
    // that order, so the caller marks the send matched before feeding the turn
    // machine the stop.
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("cmdname"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the local command consumed the outstanding send"
    );
    // The group is machinery: it never tears the turn back off `main`.
    assert_eq!(outcome.state.carry_thread, MAIN);
}

#[test]
fn a_namespaced_local_command_name_line_matches_a_short_form_send() {
    // Like the previous test, but the user typed the SHORT form `/review-pr`
    // (so Delta dispatched send 7 with that exact text) while Claude expanded it
    // to its fully-qualified namespaced form `/example:review-pr` in the
    // transcript command-name line. Consumption is positional either way; what
    // this pins is that the bare-command-name comparison recognizes the two
    // forms as the same command, so the send is reported as `attributed` and
    // raises no rewrite warning.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/example:review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert_eq!(message(&outcome, "caveat").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "cmdname").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "stdout").role, delta_model::Role::Meta);

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("cmdname"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the namespaced command-name line consumed the short-form outstanding send"
    );
    assert_eq!(outcome.state.carry_thread, MAIN);
}

#[test]
fn a_local_command_name_line_consumes_a_slash_command_send_of_another_name() {
    // Delta dispatched `/review-pr` as send 7, but the command-name line Claude
    // recorded names a DIFFERENT command (`/example:audit`) — a name rewrite
    // Delta has not catalogued, or something pre-empting the paste in the pane.
    // The send is still consumed: it was a slash command, so it produced no
    // `UserPromptSubmit` echo and no `Stop`, and this command line is the only
    // evidence it left. Deciding by name would leave send 7 wedged until the
    // echo deadline retyped the command a second time. The mismatch is
    // reported as `attributed: false` (the caller warns) rather than acted on.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/example:audit"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("cmdname"),
                attributed: false,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the command-name line consumed the outstanding slash-command send \
         despite naming another command"
    );
    assert_eq!(outcome.state.carry_thread, MAIN);
}

#[test]
fn a_local_command_name_line_leaves_a_plain_prompt_send_outstanding() {
    // The guard the positional rule needs: send 7 is a PLAIN prompt, which
    // Claude echoes back through `UserPromptSubmit` — so a local-command group
    // showing up while it is outstanding cannot be its outcome. Somebody typed
    // a command straight into the pane ahead of the send. Consuming send 7 here
    // would mark the user's message delivered and drop it, so the group folds
    // to meta and leaves the send outstanding for its own echo.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(send(7, CHILD, "hello world"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert_eq!(message(&outcome, "cmdname").role, delta_model::Role::Meta);
    assert!(
        outcome.effects.is_empty(),
        "a plain-prompt send is neither consumed nor turn-ended by a command line"
    );
    assert_eq!(
        outcome.state.outstanding.len(),
        1,
        "the plain-prompt send stays outstanding, waiting for its own echo"
    );
    assert_eq!(
        outcome.state.carry_thread, CHILD,
        "local-command machinery inherits the current thread, never resets to main"
    );
}

#[test]
fn a_local_command_with_no_outstanding_send_just_folds_to_meta() {
    // A local command typed straight into the pane (never dispatched by Delta):
    // there is no outstanding send to resolve, so the group simply folds to
    // meta with no send/turn effects — and must NOT reset attribution to main.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert_eq!(message(&outcome, "cmdname").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "stdout").role, delta_model::Role::Meta);
    assert!(
        outcome.effects.is_empty(),
        "nothing to resolve, nothing to end"
    );
    assert_eq!(
        outcome.state.carry_thread, CHILD,
        "local-command machinery inherits the current thread, never resets to main"
    );
}

#[test]
fn an_unknown_command_notice_resolves_its_send_and_ends_the_turn() {
    // The user typed `/review-pr`, but no such slash command exists. Delta
    // dispatched it as send 7, so the turn machine is AwaitingEcho{7}. Claude
    // rejects an unknown command client-side: NO UserPromptSubmit echo, NO Stop,
    // and no command group — only a `system`/informational warning
    // "Unknown command: /review-pr". Left alone send 7 wedges the queue forever,
    // exactly like a known local command. The notice must consume send 7 and end
    // the degenerate turn, in that order.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![unknown_command_notice_line("notice", "/review-pr")],
    );

    // The notice surfaces as a system line (not folded to meta, not a user turn).
    assert_eq!(message(&outcome, "notice").role, delta_model::Role::System);
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("notice"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the unknown-command notice consumed the outstanding send"
    );
    // The notice is machinery: it never tears the turn back off `main`.
    assert_eq!(outcome.state.carry_thread, MAIN);
}

#[test]
fn an_unknown_command_notice_matches_a_send_carrying_args() {
    // The dispatched send may carry args (`/review-pr 123`), while the notice
    // names only the command (`/review-pr`). The name comparison is on the
    // send's first whitespace-delimited token, so the args must not make the
    // send look unrecognized.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr 123"))),
        vec![unknown_command_notice_line("notice", "/review-pr")],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("notice"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(outcome.state.outstanding.is_empty());
}

#[test]
fn an_unknown_command_notice_consumes_a_slash_command_send_of_another_name() {
    // The unknown-notice analogue of
    // `a_local_command_name_line_consumes_a_slash_command_send_of_another_name`:
    // Delta dispatched `/review-pr 123` as send 7, and the notice names
    // `/revew-pr` — the shape this branch must expect, since Claude echoes back
    // whatever it parsed out of a command it did not recognize (extra
    // characters landing in the pane between Delta's paste and its Enter are
    // enough). The notice is still send 7's outcome, so it is consumed and the
    // degenerate turn ends; only `attributed` records that the names differ.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr 123"))),
        vec![unknown_command_notice_line("notice", "/revew-pr")],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("notice"),
                attributed: false,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the notice consumed the outstanding slash-command send despite naming \
         another command"
    );
}

#[test]
fn an_unknown_command_notice_leaves_a_plain_prompt_send_outstanding() {
    // The same guard as on the local-command branch: send 7 is a PLAIN prompt,
    // so it is echoed through `UserPromptSubmit` and an unknown-command notice
    // cannot be its outcome — it is the rejection of a command typed straight
    // into the pane. Consuming send 7 here would drop the user's message, so
    // the notice merely surfaces and the send stays outstanding.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(send(7, CHILD, "hello world"))),
        vec![unknown_command_notice_line("notice", "/revew-pr")],
    );

    assert_eq!(message(&outcome, "notice").role, delta_model::Role::System);
    assert!(
        outcome.effects.is_empty(),
        "a plain-prompt send is neither consumed nor turn-ended by a notice"
    );
    assert_eq!(
        outcome.state.outstanding.len(),
        1,
        "the plain-prompt send stays outstanding, waiting for its own echo"
    );
    assert_eq!(
        outcome.state.carry_thread, CHILD,
        "the notice inherits the current thread, never resets to main"
    );
}

#[test]
fn an_unknown_command_notice_with_no_outstanding_send_just_surfaces() {
    // An unknown command typed straight into the pane (never dispatched by
    // Delta): there is no outstanding send to resolve, so the notice surfaces as
    // a `Role::System` line with no send/turn effects — and must NOT reset
    // attribution to main.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![unknown_command_notice_line("notice", "/review-pr")],
    );

    assert_eq!(message(&outcome, "notice").role, delta_model::Role::System);
    assert!(
        outcome.effects.is_empty(),
        "nothing to resolve, nothing to end"
    );
    assert_eq!(
        outcome.state.carry_thread, CHILD,
        "the notice inherits the current thread, never resets to main"
    );
}

#[test]
fn a_queued_replay_after_compact_matches_its_send_instead_of_folding_to_meta() {
    // Repro of the post-compact hidden-queued-prompt bug: the user submits a
    // prompt while an auto- or manual `/compact` is running; the CLI buffers it
    // in the internal input queue, and after compact finishes it replays the
    // prompt as an ordinary `type: "user"` line stamped `promptSource:
    // "queued"`. Because Claude Code opens the post-compact turn on ONE
    // `promptId`, that replay shares the compact group's `promptId` — a
    // 5-line group here: the `Role::CompactSummary`, the `<local-command-caveat>`,
    // the `/compact` command-name line, the `<local-command-stdout>`, and the
    // queued replay.
    //
    // Before the fix, the replay was folded to `Role::Meta` by the local-command
    // group and its text matched the outstanding send: the fold emitted
    // `SendMatched` + `LocalCommandTurnEnded` against the replay, marking the
    // send matched but tearing the turn down as an interrupt — so delta hid
    // the user bubble AND fired `TurnInterrupted` while Claude Code's real
    // reply for the prompt streamed in.
    //
    // The fix is to exclude an `is_queued_replay` line from the group's Meta
    // reclassification: the replay flows the normal `is_human_turn` path, so
    // it matches the head outstanding send by text and lands on the send's
    // thread as `Role::User` — exactly as if no compact had happened. The
    // group's other members still fold to `Meta`; `AutoCompactFinished` still
    // fires from the compact-summary line so a caller-side re-type is still
    // available for the *other* case (a send stuck behind a swallowed echo).
    //
    // Companion assertion: `a_local_command_group_folds_to_meta_resolves_its_send_and_ends_the_turn`
    // (above) still pins that a real `/review-pr`-style group with no queued
    // replay ends the turn as before — i.e. the fix did not regress the
    // command-name send-matching path.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(send(7, CHILD, "the user's actual prompt"))),
        vec![
            with_prompt_id(
                "pcompact",
                compact_summary_line("cs-1", "<summary>of the previous conversation</summary>"),
            ),
            local_command_caveat_line("caveat", "pcompact"),
            local_command_name_line("cmdname", "pcompact", "/compact"),
            local_command_stdout_line("stdout", "pcompact"),
            with_prompt_id(
                "pcompact",
                queued_replay_line("u-replay", "the user's actual prompt"),
            ),
        ],
    );

    // The compact summary, caveat, command-name, and stdout still fold as
    // command machinery — only the queued replay is spared the group's Meta
    // reclassification.
    assert_eq!(
        message(&outcome, "cs-1").role,
        delta_model::Role::CompactSummary
    );
    assert_eq!(message(&outcome, "caveat").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "cmdname").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "stdout").role, delta_model::Role::Meta);
    // The persisted role for the queued replay is `User`, NOT `Meta` — this
    // is the regression pin for the hidden-user-bubble symptom.
    assert_eq!(
        message(&outcome, "u-replay").role,
        delta_model::Role::User,
        "the queued replay must persist as Role::User so the pane renders a \
         user bubble; folding it to Meta hides the human prompt from the UI"
    );
    // It landed on the send's thread — the branch the user actually addressed
    // the prompt to — not on `main` and not stuck on `CHILD` as an inherit.
    assert_eq!(message(&outcome, "u-replay").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);

    // The compact-summary line still emits `AutoCompactFinished`, and the
    // queued replay emits `SendMatched` for the outstanding send — but NOT
    // `LocalCommandTurnEnded`, which would tear the live turn down as an
    // interrupt while the model's real reply is still streaming in.
    assert_eq!(
        outcome.effects,
        vec![
            Effect::AutoCompactFinished,
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("u-replay"),
                attributed: true,
            },
        ]
    );
    assert!(
        !outcome
            .effects
            .iter()
            .any(|e| matches!(e, Effect::LocalCommandTurnEnded { .. })),
        "the queued replay is a genuine human turn, not command machinery — \
         it must not end the turn as if a local command had consumed the send"
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the send is consumed by the queued replay, not left dangling"
    );
}

#[test]
fn a_queued_replay_after_compact_without_a_matching_send_lands_on_main_as_user() {
    // Same shape as above, but no outstanding send: the queued replay is
    // uncorrelated external input from the fold's point of view. It must
    // still fold as `Role::User` (never `Meta`), flow through the human-turn
    // branch's `None` arm, and reset `carry_thread` to `main` like any other
    // external human line — never inheriting the compact group's Meta
    // treatment. `LocalCommandTurnEnded` must not fire because no send was
    // consumed.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            with_prompt_id(
                "pcompact",
                compact_summary_line("cs-1", "<summary>of the previous conversation</summary>"),
            ),
            local_command_caveat_line("caveat", "pcompact"),
            local_command_name_line("cmdname", "pcompact", "/compact"),
            local_command_stdout_line("stdout", "pcompact"),
            with_prompt_id(
                "pcompact",
                queued_replay_line("u-replay", "the user's actual prompt"),
            ),
        ],
    );

    assert_eq!(
        message(&outcome, "u-replay").role,
        delta_model::Role::User,
        "even without an outstanding send the queued replay stays User, never Meta"
    );
    assert_eq!(message(&outcome, "u-replay").thread_id, MAIN);
    assert_eq!(outcome.state.carry_thread, MAIN);
    // `AutoCompactFinished` still fires from the summary line; no send match,
    // no `LocalCommandTurnEnded`.
    assert_eq!(outcome.effects, vec![Effect::AutoCompactFinished]);
}

#[test]
fn a_real_prompt_after_a_local_command_is_an_ordinary_user_turn() {
    // The bare command-name line must not be confused with the human prompt
    // that follows it: a later user line with a DIFFERENT promptId is a genuine
    // turn (here matching its own send 8), proving the local-command grouping
    // is scoped to the caveat's promptId and does not leak forward.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState {
            outstanding: vec![send(7, MAIN, "/review-pr"), send(8, MAIN, "now review it")].into(),
            ..AttributionState::new(MAIN, None)
        },
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
            with_prompt_id("p-real", user_line("u-real", "now review it")),
        ],
    );

    // The local command consumed send 7 and ended that turn; the real prompt
    // consumed send 8 as an ordinary human turn.
    assert_eq!(message(&outcome, "u-real").role, delta_model::Role::User);
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("cmdname"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
            Effect::SendMatched {
                send_id: 8,
                matched_uuid: MessageUuid::from("u-real"),
                attributed: true,
            },
        ]
    );
    assert!(outcome.state.outstanding.is_empty());
}

// --- Forked skills -------------------------------------------------------------

#[test]
fn a_forked_skill_launch_lights_a_background_indicator_and_records_the_launch() {
    // The reported bug: a session started with a slash command that forks its
    // skill into a background agent (`/review-pr`, recorded as
    // `/example:review-pr`) showed NO running indicator for the minutes
    // the skill worked. The forked skill is launched by the CLI harness, not
    // by the model, so the parent transcript carries no `tool_use` block at
    // all — only the `<forked-skill-launch>` element on the local-command
    // system line. Folding that element must record the launch (so the later
    // `<task-notification>` correlates back) AND light a BACKGROUND indicator
    // (so it survives the degenerate turn end the same group emits).
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/example:review-pr"),
            forked_skill_launch_line("forked", "a7046b32df40e1b3e", "example:review-pr"),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("cmdname"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
            Effect::SubagentLaunched {
                tool_use_id: "forked-skill:a7046b32df40e1b3e".into(),
                thread_id: MAIN,
                // The forked skill knows its background-task id up front: the
                // payload's `agentId` IS the `<task-id>` its completion
                // notification will carry.
                task_id: Some("a7046b32df40e1b3e".into()),
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "forked-skill:a7046b32df40e1b3e".into(),
                thread_id: MAIN,
                subagent_type: Some("example:review-pr".into()),
                description: Some("/example:review-pr".into()),
                background: true,
            },
        ]
    );
    // The launch is seeded with its task_id, so a notification folded in the
    // SAME window matches by `<task-id>` without any store round-trip.
    assert_eq!(
        outcome
            .state
            .launched_threads
            .get("forked-skill:a7046b32df40e1b3e")
            .and_then(|launch| launch.task_id.clone()),
        Some("a7046b32df40e1b3e".into())
    );
    // The launch line is machinery: it never tears the turn off its thread.
    assert_eq!(outcome.state.carry_thread, MAIN);
}

#[test]
fn a_forked_skill_launch_is_attributed_to_the_launching_thread() {
    // The launching thread is `carry_thread` — the thread the group's lines and
    // the later `<task-notification>` are attributed to — so the indicator, the
    // messages and the unread suppression all agree even when the command was
    // run from a sub-thread.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![forked_skill_launch_line(
            "forked",
            "agent-1",
            "example:review-pr",
        )],
    );

    assert_eq!(message(&outcome, "forked").thread_id, CHILD);
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "forked-skill:agent-1".into(),
                thread_id: CHILD,
                task_id: Some("agent-1".into()),
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "forked-skill:agent-1".into(),
                thread_id: CHILD,
                subagent_type: Some("example:review-pr".into()),
                description: Some("/example:review-pr".into()),
                background: true,
            },
        ]
    );
}

#[test]
fn a_forked_skill_completion_notification_clears_the_launch_by_task_id() {
    // The real forked-skill completion shape: a `<task-notification>` carrying
    // only `<task-id>` (the `agentId`), no `<tool-use-id>` — there never was a
    // tool_use. It must resolve against the launch seeded above and emit
    // `SubagentCompleted` keyed by the SAME synthetic id the indicator was lit
    // under, so the running entry is finished.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            forked_skill_launch_line("forked", "agent-1", "example:review-pr"),
            task_notification_line_with_task_id_only("u-note", "agent-1"),
            assistant_line("a-after", "the review landed"),
        ],
    );

    assert_eq!(
        outcome.effects.last(),
        Some(&Effect::SubagentCompleted {
            tool_use_id: "forked-skill:agent-1".into(),
        })
    );
    assert!(
        outcome.state.launched_threads.is_empty(),
        "the completion consumed the forked-skill launch"
    );
}

#[test]
fn a_line_with_no_forked_skill_launch_element_emits_no_launch_effects() {
    // The ordinary local-command group — a slash command that does NOT fork a
    // skill — must stay exactly as it was: a degenerate finished turn with no
    // subagent effects whatsoever.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert!(
        outcome.effects.is_empty(),
        "a local command with no forked-skill launch emits nothing, got {:?}",
        outcome.effects
    );
    assert!(outcome.state.launched_threads.is_empty());
}

#[test]
fn a_forked_skill_launch_with_an_unusable_body_emits_no_launch_effects() {
    // Both degenerate bodies: unparsable JSON, and JSON naming no `agentId`.
    // Without the correlation key nothing can be tracked, so the fold must
    // stay silent rather than mint a launch that can never be completed.
    for body in [
        "not json at all",
        r#"{"skillName":"example:review-pr"}"#,
        r#"{"agentId":""}"#,
    ] {
        let outcome = attribute_lines(
            &session(),
            MAIN,
            AttributionState::new(MAIN, None),
            vec![forked_skill_launch_line_with_body("forked", body)],
        );
        assert!(
            outcome.effects.is_empty(),
            "body {body:?} must emit no effects, got {:?}",
            outcome.effects
        );
        assert!(outcome.state.launched_threads.is_empty());
    }
}

#[test]
fn a_quoted_forked_skill_element_on_a_human_or_assistant_line_mints_no_launch() {
    // The payload is plain text, so a human prompt or an assistant reply can
    // QUOTE it — and in practice does: Delta is developed inside Delta, and its
    // own task docs and test fixtures carry a verbatim
    // `<forked-skill-launch>` element that a session working on this repo
    // routinely reads back into the transcript. Only the harness-written
    // `local_command` line (`Role::Meta` after parsing) may light an indicator:
    // a launch minted from a quote would be BACKGROUND, so it would survive
    // every turn-end sweep and stay lit until the session closed, with no
    // `<task-notification>` able to ever clear it.
    let quoted = "look at this line: <forked-skill-launch>\
                  {\"agentId\":\"a7046b32df40e1b3e\",\
                  \"skillName\":\"example:review-pr\"}</forked-skill-launch>";
    for line in [
        user_line("u-quote", quoted),
        assistant_line("a-quote", quoted),
    ] {
        let uuid = line.uuid.clone();
        let outcome = attribute_lines(
            &session(),
            MAIN,
            AttributionState::new(MAIN, None),
            vec![line],
        );
        assert!(
            outcome.effects.is_empty(),
            "a quoted element on {uuid:?} must emit no effects, got {:?}",
            outcome.effects
        );
        assert!(outcome.state.launched_threads.is_empty());
    }
}
