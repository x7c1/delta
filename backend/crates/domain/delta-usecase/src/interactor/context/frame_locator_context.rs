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

#[cfg(test)]
mod tests {
    use super::frame_locator_context;

    /// Wraps a non-empty quote with provenance framing and the quote delimited
    /// so the frame and the quote stay distinguishable.
    #[test]
    fn frames_a_quote() {
        let framed =
            frame_locator_context("the main channel").expect("a non-empty quote is framed");
        assert_eq!(
            framed,
            "The user is replying to this passage they selected from earlier in the conversation:\n\"the main channel\""
        );
    }

    /// Surrounding whitespace is trimmed before the quote is framed.
    #[test]
    fn trims_the_quote() {
        let framed = frame_locator_context("  spaced  ").expect("a non-blank quote is framed");
        assert_eq!(
            framed,
            "The user is replying to this passage they selected from earlier in the conversation:\n\"spaced\""
        );
    }

    /// An empty or whitespace-only quote yields `None`, so nothing is injected.
    #[test]
    fn returns_none_for_blank_quote() {
        assert!(frame_locator_context("").is_none());
        assert!(frame_locator_context("   \n\t ").is_none());
    }

    /// A selected passage may itself contain double quotes and span multiple lines.
    /// Only the surrounding whitespace is trimmed; the interior is embedded
    /// verbatim, with no escaping of the delimiters. The frame is a prose hint for
    /// the model, not a strict grammar, so this pins the shipped behaviour down
    /// rather than asserting any escaping.
    #[test]
    fn embeds_quotes_and_newlines_verbatim() {
        let framed = frame_locator_context("  she said \"go\"\nthen left  ")
            .expect("a non-blank quote is framed");
        assert_eq!(
            framed,
            "The user is replying to this passage they selected from earlier in the conversation:\n\"she said \"go\"\nthen left\""
        );
    }
}
