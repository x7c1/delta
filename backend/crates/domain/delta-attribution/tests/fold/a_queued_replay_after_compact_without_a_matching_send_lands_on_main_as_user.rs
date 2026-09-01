use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

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
