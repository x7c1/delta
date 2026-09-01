use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

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
