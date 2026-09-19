//! The `<pasted_content>` wrapper Claude Code puts around pasted text.
//!
//! Recent Claude Code builds wrap text that arrives as a paste (and whose
//! trimmed length reaches a threshold, observed as 20 characters) in a tag
//! pair before submitting the prompt:
//!
//! ```text
//! \n\n<pasted_content id="d626">\n<body>\n</pasted_content id="d626">\n
//! ```
//!
//! Both the `UserPromptSubmit` hook's `prompt` and the transcript's human line
//! carry this form, while the Claude Code TUI shows neither the tags nor the
//! id. Delta types every send as an xterm bracketed paste, so every send long
//! enough is echoed wrapped. The wrapper is transport, not content: this
//! module recognizes it so the echo comparison can see through it and the
//! stored human message can drop it. The shape it accepts is spelled out on
//! [`unwrap_pasted_content`].

use std::borrow::Cow;

/// The opening tag up to its id: `<pasted_content id="`.
const OPEN_TAG_PREFIX: &str = "<pasted_content id=\"";

/// The closing tag up to its id: `</pasted_content id="`. Unlike an HTML end
/// tag, Claude Code repeats the id on the closing tag.
const CLOSE_TAG_PREFIX: &str = "</pasted_content id=\"";

/// What follows the id in both tags.
const TAG_SUFFIX: &str = "\">";

/// Length of the per-session id, in (ASCII hex) characters.
const ID_LEN: usize = 4;

/// The most newlines Claude Code puts in front of a block. Observed on a
/// prompt that is nothing but the block: `\n\n` precedes the opening tag.
const MAX_NEWLINES_BEFORE_BLOCK: usize = 2;

/// The most newlines Claude Code puts after a block. Observed on the same
/// prompt: one `\n` follows the closing tag.
const MAX_NEWLINES_AFTER_BLOCK: usize = 1;

/// A well-formed block found in a text.
struct Block<'a> {
    /// Byte offset of the opening tag's `<`.
    start: usize,
    /// Byte offset just past the closing tag's `>`.
    end: usize,
    /// The pasted text, without the newlines the wrapper added around it.
    body: &'a str,
}

/// If `prompt` is exactly one well-formed block (surrounding whitespace
/// ignored), the pasted body it wraps. `None` for a prompt with no block, a
/// malformed block, or anything outside the block — text typed before or
/// after it, or a second block.
pub(super) fn sole_pasted_content_body(prompt: &str) -> Option<&str> {
    let trimmed = prompt.trim();
    let block = block_at(trimmed, 0)?;
    (block.end == trimmed.len()).then_some(block.body)
}

/// `text` with every well-formed block replaced by its pasted body. The tags
/// go, and so do the newlines the wrapper added: the one after the opening tag,
/// the one before the closing tag, and — as observed on a prompt that is
/// nothing but the block — up to two in front of the opening tag and one after
/// the closing tag. Text typed next to a block is kept.
///
/// A block is **well-formed** when:
///
/// - the opening tag is `<pasted_content id="XXXX">` and the closing tag is
///   `</pasted_content id="XXXX">`, with the same id in both, the id being
///   exactly four lowercase hex characters;
/// - the opening tag is followed by a newline, and the closing tag is preceded
///   by one (Claude Code adds it unless the body already ends with a newline,
///   so the two cases produce the same bytes and one newline is always
///   removed);
/// - the body holds no other opening or closing tag. Claude Code escapes a
///   tag-like `<pasted_content` inside a pasted body to `<\pasted_content`, so
///   a real body never does.
///
/// Anything else — mismatched ids, a malformed id, a missing closing tag, a
/// missing newline — is left alone: a malformed block reads as the text it
/// is. Borrows `text` unchanged when it holds no well-formed block.
pub fn unwrap_pasted_content(text: &str) -> Cow<'_, str> {
    let mut out = String::new();
    // Byte offset up to which `text` has been copied into `out` (or consumed).
    let mut copied = 0;
    let mut search_from = 0;
    while let Some(found) = text[search_from..].find(OPEN_TAG_PREFIX) {
        let start = search_from + found;
        let Some(block) = block_at(text, start) else {
            // Malformed: keep it verbatim and look for the next opening tag.
            search_from = start + OPEN_TAG_PREFIX.len();
            continue;
        };
        out.push_str(strip_trailing_newlines(
            &text[copied..block.start],
            MAX_NEWLINES_BEFORE_BLOCK,
        ));
        out.push_str(block.body);
        copied = block.end + leading_newlines(&text[block.end..], MAX_NEWLINES_AFTER_BLOCK);
        search_from = copied;
    }
    if copied == 0 {
        return Cow::Borrowed(text);
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

/// Parse the well-formed block whose opening tag starts at byte `start` of
/// `text`. `None` when no opening tag starts there or the block is malformed.
fn block_at(text: &str, start: usize) -> Option<Block<'_>> {
    let after_prefix = text[start..].strip_prefix(OPEN_TAG_PREFIX)?;
    let id = after_prefix.get(..ID_LEN)?;
    if !id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let content = after_prefix[ID_LEN..]
        .strip_prefix(TAG_SUFFIX)?
        .strip_prefix('\n')?;
    // The FIRST closing tag must be this block's own: a closing tag with
    // another id before it means the block is not well-formed.
    let close_at = content.find(CLOSE_TAG_PREFIX)?;
    let close_tag = &content[close_at..];
    let after_close_id = close_tag[CLOSE_TAG_PREFIX.len()..].strip_prefix(id)?;
    if !after_close_id.starts_with(TAG_SUFFIX) {
        return None;
    }
    let body = content[..close_at].strip_suffix('\n')?;
    if body.contains(OPEN_TAG_PREFIX) {
        return None;
    }
    let close_len = CLOSE_TAG_PREFIX.len() + ID_LEN + TAG_SUFFIX.len();
    let end = text.len() - close_tag.len() + close_len;
    Some(Block { start, end, body })
}

/// `text` with up to `max` newlines removed from its end.
fn strip_trailing_newlines(text: &str, max: usize) -> &str {
    let mut rest = text;
    for _ in 0..max {
        match rest.strip_suffix('\n') {
            Some(stripped) => rest = stripped,
            None => break,
        }
    }
    rest
}

/// How many bytes of newlines (at most `max`) `text` opens with.
fn leading_newlines(text: &str, max: usize) -> usize {
    text.bytes().take(max).take_while(|&b| b == b'\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Observed verbatim: a two-line send, echoed wrapped.
    const WRAPPED_MULTI_LINE: &str = "\n\n<pasted_content id=\"d626\">\nこれって今どこまで進んでますか\nそれともこれから開始するところですか\n</pasted_content id=\"d626\">\n";

    /// Observed verbatim: a single-line send of 20+ characters, echoed wrapped.
    const WRAPPED_SINGLE_LINE: &str = "\n\n<pasted_content id=\"d626\">\n実装に着手してほしいのですが、その前に認証をこちらで済ませておかないといけない、という理解であっていますか\n</pasted_content id=\"d626\">\n";

    #[test]
    fn the_sole_body_of_an_observed_prompt_is_the_pasted_text() {
        assert_eq!(
            sole_pasted_content_body(WRAPPED_MULTI_LINE),
            Some("これって今どこまで進んでますか\nそれともこれから開始するところですか")
        );
        assert_eq!(
            sole_pasted_content_body(WRAPPED_SINGLE_LINE),
            Some("実装に着手してほしいのですが、その前に認証をこちらで済ませておかないといけない、という理解であっていますか")
        );
    }

    #[test]
    fn a_body_ending_in_a_newline_loses_only_the_one_before_the_closing_tag() {
        assert_eq!(
            sole_pasted_content_body(
                "<pasted_content id=\"0a1f\">\nfirst line\n\n</pasted_content id=\"0a1f\">"
            ),
            Some("first line\n")
        );
    }

    #[test]
    fn malformed_blocks_have_no_sole_body() {
        for prompt in [
            // Mismatched open/close ids.
            "<pasted_content id=\"d626\">\nbody\n</pasted_content id=\"d627\">",
            // Uppercase, non-hex, too short, too long ids.
            "<pasted_content id=\"D626\">\nbody\n</pasted_content id=\"D626\">",
            "<pasted_content id=\"zz26\">\nbody\n</pasted_content id=\"zz26\">",
            "<pasted_content id=\"d62\">\nbody\n</pasted_content id=\"d62\">",
            "<pasted_content id=\"d6260\">\nbody\n</pasted_content id=\"d6260\">",
            // Missing closing tag.
            "<pasted_content id=\"d626\">\nbody\n",
            // Missing newline after the opening tag / before the closing tag.
            "<pasted_content id=\"d626\">body\n</pasted_content id=\"d626\">",
            "<pasted_content id=\"d626\">\nbody</pasted_content id=\"d626\">",
            // A nested opening tag inside the body.
            "<pasted_content id=\"d626\">\n<pasted_content id=\"d626\">\n</pasted_content id=\"d626\">",
            // No wrapper at all.
            "plain text",
        ] {
            assert_eq!(
                sole_pasted_content_body(prompt),
                None,
                "expected {prompt:?} to be refused"
            );
        }
    }

    #[test]
    fn text_outside_the_block_means_it_is_not_the_sole_body() {
        let block = "<pasted_content id=\"d626\">\nbody\n</pasted_content id=\"d626\">";
        assert_eq!(sole_pasted_content_body(&format!("{block}\nmore")), None);
        assert_eq!(sole_pasted_content_body(&format!("intro\n\n{block}")), None);
        assert_eq!(sole_pasted_content_body(&format!("{block}\n{block}")), None);
    }

    #[test]
    fn unwrapping_an_observed_prompt_yields_the_send_text() {
        assert_eq!(
            unwrap_pasted_content(WRAPPED_MULTI_LINE),
            "これって今どこまで進んでますか\nそれともこれから開始するところですか"
        );
    }

    #[test]
    fn unwrapping_keeps_typed_text_around_a_block() {
        let text = "please look at this\n\n<pasted_content id=\"3b9c\">\nline one\nline two\n</pasted_content id=\"3b9c\">\n\nwhat do you think?";
        assert_eq!(
            unwrap_pasted_content(text),
            "please look at thisline one\nline two\nwhat do you think?"
        );
    }

    #[test]
    fn unwrapping_replaces_every_well_formed_block() {
        let text = "\n\n<pasted_content id=\"3b9c\">\nfirst\n</pasted_content id=\"3b9c\">\n and \n\n<pasted_content id=\"3b9c\">\nsecond\n</pasted_content id=\"3b9c\">\n";
        assert_eq!(unwrap_pasted_content(text), "first and second");
    }

    #[test]
    fn unwrapping_leaves_malformed_blocks_and_plain_text_as_they_are() {
        for text in [
            "plain text",
            "<pasted_content id=\"d626\">\nbody\n</pasted_content id=\"d627\">",
            "<pasted_content id=\"d626\">\nno closing tag",
            "a <\\pasted_content id=\"d626\"> escaped tag",
        ] {
            assert!(
                matches!(unwrap_pasted_content(text), Cow::Borrowed(t) if t == text),
                "expected {text:?} to be left alone"
            );
        }
        // A malformed block next to a well-formed one: only the latter unwraps.
        let text = "<pasted_content id=\"XYZW\">\nkept\n</pasted_content id=\"XYZW\">\n\n<pasted_content id=\"d626\">\nbody\n</pasted_content id=\"d626\">\n";
        assert_eq!(
            unwrap_pasted_content(text),
            "<pasted_content id=\"XYZW\">\nkept\n</pasted_content id=\"XYZW\">body"
        );
    }
}
