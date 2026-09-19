use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::{ContentBlock, MessageUuid};

use crate::support::*;

/// The send Delta typed (as a bracketed paste).
const SEND_TEXT: &str = "これって今どこまで進んでますか\nそれともこれから開始するところですか";

/// What Claude Code recorded for it: the pasted body wrapped in its
/// `<pasted_content>` tag pair (observed verbatim).
const WRAPPED_ECHO: &str = "\n\n<pasted_content id=\"d626\">\nこれって今どこまで進んでますか\nそれともこれから開始するところですか\n</pasted_content id=\"d626\">\n";

#[test]
fn a_pasted_content_echo_matches_its_send_and_is_stored_unwrapped() {
    // Claude Code wraps a long enough paste in a tag pair before submitting it.
    // The wrapper is transport: the echo still reads as the send's own text
    // (`attributed: true`), and the stored human message is what the user
    // wrote, with no tag left for the conversation pane to show.
    let pending = branch_send(7, CHILD, "uuid-parent", SEND_TEXT);
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(pending)),
        vec![
            user_line("u-b", WRAPPED_ECHO),
            assistant_line("a-b", "branch reply"),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![Effect::SendMatched {
            send_id: 7,
            matched_uuid: MessageUuid::from("u-b"),
            attributed: true,
        }]
    );
    let echo = message(&outcome, "u-b");
    assert_eq!(echo.thread_id, CHILD);
    assert_eq!(echo.content_text.as_deref(), Some(SEND_TEXT));
    assert_eq!(
        echo.content,
        vec![ContentBlock::Text {
            text: SEND_TEXT.into()
        }]
    );
}
