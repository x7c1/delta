//! Attribution-fold semantics, tested directly on the pure function.
//!
//! These pin the line-classification and thread-assignment rules without any
//! store or actor: ports of the logic-heavy parts of the `delta-usecase`
//! sync test suite, plus the seams that suite could not reach (effect order,
//! send non-consumption, state threading).

mod support;

use delta_attribution::{attribute_lines, AttributionState, Effect};
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
        }]
    );
    assert!(outcome.state.outstanding.is_empty(), "the match consumed the send");
}

#[test]
fn a_branch_send_echo_attributes_user_and_assistant_to_the_child() {
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(branch_send(7, CHILD, "uuid-parent", "branch text"))),
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
    // in docs/guides/development.md): a branch send issued while a turn was
    // in flight was queued by Claude and recorded only as a `queued_command`
    // attachment — never a normal user line. It must still correlate to its
    // send so the prompt AND the reply land on the child thread, not `main`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(branch_send(7, CHILD, "uuid-parent", "branch text"))),
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
    // in docs/guides/development.md): a queued command that matches no send —
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
        vec![unrelated].into_iter().collect::<std::collections::VecDeque<_>>(),
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
        vec![unrelated].into_iter().collect::<std::collections::VecDeque<_>>(),
        "the api-error line must not match or consume the pending send"
    );
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
        vec![unrelated].into_iter().collect::<std::collections::VecDeque<_>>(),
        "the notification must not match or consume the pending send"
    );
}

#[test]
fn an_external_human_line_resets_carry_to_main_without_consuming_the_send() {
    // A human user line matching no outstanding send is external input: it
    // lands on `main` and resets `carry_thread` — but the non-matching
    // outstanding send stays dispatched, so its echo can still match later.
    let pending = branch_send(7, CHILD, "uuid-parent", "branch text");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(pending)),
        vec![
            user_line("u-ext", "typed straight into the pane"),
            assistant_line("a-ext", "external reply"),
            user_line("u-b", "branch text"),
        ],
    );

    assert_eq!(message(&outcome, "u-ext").thread_id, MAIN);
    assert_eq!(message(&outcome, "a-ext").thread_id, MAIN);
    // The send survived the external turn and matched its real echo.
    assert_eq!(message(&outcome, "u-b").thread_id, CHILD);
    assert_eq!(
        outcome.effects,
        vec![Effect::SendMatched {
            send_id: 7,
            matched_uuid: MessageUuid::from("u-b"),
        }]
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
fn send_matching_compares_trimmed_text() {
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, CHILD, "  spaced prompt \n"))),
        vec![user_line("u-1", "spaced prompt")],
    );

    assert_eq!(message(&outcome, "u-1").thread_id, CHILD);
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
