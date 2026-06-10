use super::delimit_quote;

/// Frame a locator quote for injection as `additionalContext`.
///
/// The locator quote is a passage the user selected from earlier in the
/// conversation to anchor their current message. Injecting it verbatim gives
/// the model no provenance, so it may read the bare text as new content or a
/// fresh instruction. This wraps it in a short frame that supplies that missing
/// provenance, with the quote delimited so the frame and the quote stay
/// distinguishable.
///
/// The frame is authorship-neutral: the selected passage may come from either an
/// assistant or a user message, so it does not claim who said it. An empty or
/// whitespace-only quote carries no content to anchor, so it yields `None` and
/// nothing is injected.
///
/// Isolated deliberately so the exact wording is easy to tune. This affects only
/// the model-facing `additionalContext`; it never changes the on-screen message
/// or any stored field.
pub(in crate::interactor) fn frame_locator_context(quote: &str) -> Option<String> {
    let quote = quote.trim();
    if quote.is_empty() {
        return None;
    }
    Some(format!(
        "The user is replying to this passage they selected from earlier in the conversation:\n{}",
        delimit_quote(quote)
    ))
}
