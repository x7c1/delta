use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

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
