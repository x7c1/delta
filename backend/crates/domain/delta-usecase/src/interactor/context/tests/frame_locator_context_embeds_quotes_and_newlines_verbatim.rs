use crate::interactor::context::frame_locator_context;

/// A selected passage may itself contain double quotes and span multiple lines.
/// Only the surrounding whitespace is trimmed; the interior is embedded
/// verbatim, with no escaping of the delimiters. The frame is a prose hint for
/// the model, not a strict grammar, so this pins the shipped behaviour down
/// rather than asserting any escaping.
#[test]
fn frame_locator_context_embeds_quotes_and_newlines_verbatim() {
    let framed = frame_locator_context("  she said \"go\"\nthen left  ")
        .expect("a non-blank quote is framed");
    assert_eq!(
        framed,
        "The user is replying to this passage they selected from earlier in the conversation:\n\"she said \"go\"\nthen left\""
    );
}
