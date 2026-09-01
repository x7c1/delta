use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

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
