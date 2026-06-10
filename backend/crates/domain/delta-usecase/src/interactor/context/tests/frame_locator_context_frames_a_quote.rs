use crate::interactor::context::frame_locator_context;

/// `frame_locator_context` wraps a non-empty quote with provenance framing and
/// the quote delimited so the frame and the quote stay distinguishable.
#[test]
fn frame_locator_context_frames_a_quote() {
    let framed = frame_locator_context("the main channel").expect("a non-empty quote is framed");
    assert_eq!(
        framed,
        "The user is replying to this passage they selected from earlier in the conversation:\n\"the main channel\""
    );
}
