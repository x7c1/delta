use crate::interactor::context::frame_locator_context;

/// Surrounding whitespace is trimmed before the quote is framed.
#[test]
fn frame_locator_context_trims_the_quote() {
    let framed = frame_locator_context("  spaced  ").expect("a non-blank quote is framed");
    assert_eq!(
        framed,
        "The user is replying to this passage they selected from earlier in the conversation:\n\"spaced\""
    );
}
