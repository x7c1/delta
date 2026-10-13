//! Whether a submitted prompt is the echo of the message Delta typed into the
//! pane.

use super::pasted_content::sole_pasted_content_body;

/// Opening of the placeholder Claude Code substitutes for an image attachment,
/// e.g. `[Image #2]`. Claude Code's composer spots an image file path in the
/// text typed into it, reads the file, and replaces the path with this
/// placeholder — hoisted to the FRONT of the submitted prompt — before the
/// prompt reaches the `UserPromptSubmit` hook and the transcript. The number
/// is a session-wide counter, not a per-message index, so it says nothing
/// about how many attachments this message carries.
const IMAGE_PLACEHOLDER_OPEN: &str = "[Image #";

/// File extensions whose paths Claude Code turns into an `[Image #N]`
/// attachment. Deliberately narrow — the same media types the placeholder is
/// emitted for — so that stripping a "path line" out of a send stays a
/// conservative operation rather than a guess about arbitrary text.
const ATTACHMENT_IMAGE_EXTENSIONS: [&str; 5] = [".png", ".jpg", ".jpeg", ".gif", ".webp"];

/// Whether a `UserPromptSubmit` prompt (or a transcript user line) is the echo
/// of `send_text` — the message Delta typed into the pane.
///
/// Plain text echoes back byte-for-byte, so the primary rule is what it has
/// always been: exact equality after trimming. Two rewrites Claude Code applies
/// between the keystrokes and the submission are recognized on top of it.
///
/// An **image-attachment** send does not echo verbatim: the composed text
/// carries the attachment's path on its own line, and Claude Code's composer
/// swallows that path, reads the file, and submits `[Image #N]<body>` instead —
/// so exact equality can never hold and the send would be treated as unechoed
/// forever. That rule strips the leading `[Image #N]` placeholders from the
/// prompt, strips the attachment path lines from the send, and compares what
/// is left.
///
/// A **pasted-content** send does not either: Delta types every send as a
/// bracketed paste, and recent Claude Code builds wrap pasted text (20 or more
/// trimmed characters, as observed) in
/// `<pasted_content id="XXXX">` … `</pasted_content id="XXXX">` before
/// submitting it (see [`unwrap_pasted_content`] for the exact shape). That rule
/// accepts a prompt that is exactly one well-formed wrapper block, surrounding
/// whitespace ignored, and compares the block's body with the send instead of
/// the prompt.
///
/// The two rewrites can meet on one prompt — an attachment send long enough to
/// be wrapped — and the order Claude Code applies them in has not been
/// observed, so both nestings are accepted: the placeholders in front of a
/// sole block (`[Image #N]` then the wrapper), and the placeholders inside the
/// block's body (the wrapper around `[Image #N]<body>`).
///
/// Every rule is deliberately conservative — a mismatch is always the safe
/// answer here, because a false *positive* would attribute someone else's
/// typing to the user's composed message:
///
/// - an attachment echo needs at least one placeholder, and the number of
///   placeholders must equal the number of path lines removed (a
///   partially-recognized attachment set does not match);
/// - a path line must be absolute, carry an image extension, and contain no
///   unescaped whitespace;
/// - the remaining attachment bodies must be equal line-for-line (each line
///   trimmed, blank lines dropped) — Claude Code drops the newline that
///   separated the body from the path, so the comparison cannot be raw
///   equality, but it is not loosened any further than that;
/// - a wrapper must be well-formed (matching open/close ids of four lowercase
///   hex characters, the wrapper's own newlines in place) and must be the
///   whole prompt: a malformed or partial block, a second block, or text typed
///   before or after the block is a mismatch. Claude Code escapes a tag-like
///   `<pasted_content` inside the pasted body to `<\pasted_content`; that is
///   not reversed, so a send whose own text contains the tag compares as a
///   mismatch.
///
/// A send with neither rewrite therefore takes exactly the old exact-match
/// path. Slash commands are untouched as well: a local-command name line is
/// resolved in its own branch at the call site, guarded by
/// [`is_slash_command_send`] and reported on by
/// [`local_command_name_line_matches_send`].
///
/// [`unwrap_pasted_content`]: super::unwrap_pasted_content
/// [`is_slash_command_send`]: super::is_slash_command_send
/// [`local_command_name_line_matches_send`]: super::local_command_name_line_matches_send
pub fn prompt_echoes_send(send_text: &str, prompt: &str) -> bool {
    echoes_without_wrapper(send_text, prompt)
        || sole_pasted_content_body(prompt)
            .is_some_and(|body| echoes_without_wrapper(send_text, body))
}

/// [`prompt_echoes_send`] for a prompt already taken out of any pasted-content
/// wrapper: the exact-match fast path, then the attachment rewrite.
fn echoes_without_wrapper(send_text: &str, prompt: &str) -> bool {
    send_text.trim() == prompt.trim() || attachment_echo_matches_send(send_text, prompt)
}

/// The attachment-aware half of [`prompt_echoes_send`]. Separate so the
/// exact-match fast path stays obvious at the call site above.
fn attachment_echo_matches_send(send_text: &str, prompt: &str) -> bool {
    let (placeholders, prompt_body) = strip_leading_image_placeholders(prompt);
    if placeholders == 0 {
        return false;
    }
    // The placeholders may sit in front of a pasted-content block rather than
    // inside it (see `prompt_echoes_send`); the block must then be all that
    // follows them.
    let prompt_body = sole_pasted_content_body(prompt_body).unwrap_or(prompt_body);
    let mut attachments = 0;
    let mut send_body = Vec::new();
    for line in body_lines(send_text) {
        if is_attachment_path_line(line) {
            attachments += 1;
        } else {
            send_body.push(line);
        }
    }
    attachments == placeholders && send_body == body_lines(prompt_body)
}

/// Split a prompt into the count of `[Image #N]` placeholders it opens with and
/// the body that follows them. Only LEADING placeholders count: that is where
/// Claude Code puts them, and refusing to scan the whole text keeps a body that
/// merely mentions the placeholder from being mistaken for an attachment.
fn strip_leading_image_placeholders(prompt: &str) -> (usize, &str) {
    let mut rest = prompt.trim_start();
    let mut count = 0;
    while let Some(after) = strip_image_placeholder(rest) {
        count += 1;
        rest = after.trim_start();
    }
    (count, rest)
}

/// Strip one `[Image #<digits>]` placeholder from the front of `text`,
/// returning the remainder. `None` when the text does not open with one.
fn strip_image_placeholder(text: &str) -> Option<&str> {
    let rest = text.strip_prefix(IMAGE_PLACEHOLDER_OPEN)?;
    let end = rest.find(|c: char| !c.is_ascii_digit())?;
    if end == 0 {
        return None;
    }
    rest[end..].strip_prefix(']')
}

/// The comparable body of a text: its lines trimmed, with blank lines dropped.
///
/// Claude Code's rewrite does not preserve the whitespace around the swallowed
/// path (the newline that separated it from the body disappears), so the two
/// sides are compared as line sequences rather than as raw strings.
fn body_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

/// Whether a (trimmed) line of a send is the path of an image attachment
/// Claude Code would swallow: an absolute path, with every space
/// backslash-escaped (the shell-escaped form a path dragged out of a file
/// manager takes), naming one of [`ATTACHMENT_IMAGE_EXTENSIONS`].
fn is_attachment_path_line(line: &str) -> bool {
    if !line.starts_with('/') {
        return false;
    }
    // Reject any whitespace that is not a backslash-escaped space: a line with
    // bare spaces is prose that happens to start with a slash, not a path.
    let mut escaped = false;
    for ch in line.chars() {
        if ch.is_whitespace() && !(ch == ' ' && escaped) {
            return false;
        }
        escaped = ch == '\\' && !escaped;
    }
    let lower = line.to_ascii_lowercase();
    ATTACHMENT_IMAGE_EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_echo_matching_is_exact_equality_after_trimming() {
        assert!(prompt_echoes_send("hello world", "hello world"));
        assert!(prompt_echoes_send("  hello world\n", "hello world"));
        assert!(!prompt_echoes_send("hello world", "hello  world"));
        assert!(!prompt_echoes_send("hello world", "typed straight in"));
        // A body that merely mentions the placeholder is still plain text: with
        // no attachment path line in the send there is nothing to strip, so the
        // exact-match semantics decide (and here they disagree).
        assert!(!prompt_echoes_send(
            "what does [Image #2] mean",
            "[Image #2] mean"
        ));
    }

    #[test]
    fn image_attachment_echo_matches_the_send_that_carried_its_path() {
        // The observed incident shape: Delta types the body plus the attachment
        // path (shell-escaped spaces) on its own line; Claude Code swallows the
        // path, reads the file, and submits the body behind an `[Image #N]`
        // placeholder — with the separating newline gone.
        let send = "can you read this picture\n/home/dev/pictures/Screenshot\\ 2026-08-09\\ at\\ 11.51.52.png";
        assert!(prompt_echoes_send(
            send,
            "[Image #2]can you read this picture"
        ));
        // The placeholder may be followed by whitespace instead of running
        // straight into the body.
        assert!(prompt_echoes_send(
            send,
            "[Image #2]\n can you read this picture"
        ));
        // A different body is still a mismatch: only the attachment rewrite is
        // tolerated, never the message itself changing.
        assert!(!prompt_echoes_send(
            send,
            "[Image #2]something else entirely"
        ));
    }

    #[test]
    fn multiple_attachments_match_when_every_path_has_a_placeholder() {
        let send = "compare these\n/home/dev/pictures/before.png\n/home/dev/pictures/after.jpeg";
        assert!(prompt_echoes_send(
            send,
            "[Image #3][Image #4]compare these"
        ));
        assert!(prompt_echoes_send(
            send,
            "[Image #3] [Image #4] compare these"
        ));
        // Fewer placeholders than path lines: Claude recognized only part of
        // the attachment set, so the correlation stays conservative and refuses.
        assert!(!prompt_echoes_send(send, "[Image #3]compare these"));
    }

    #[test]
    fn a_send_that_is_only_an_attachment_path_matches_the_bare_placeholder() {
        assert!(prompt_echoes_send(
            "/home/dev/pictures/diagram.png",
            "[Image #1]"
        ));
    }

    #[test]
    fn attachment_matching_refuses_lines_that_are_not_image_paths() {
        // Relative path, no image extension, and prose starting with a slash
        // are all left in the body, so nothing is stripped and the exact-match
        // rule decides (it disagrees).
        for send in [
            "look\npictures/diagram.png",
            "look\n/home/dev/notes.txt",
            "look\n/home/dev is where it lives.png",
        ] {
            assert!(
                !prompt_echoes_send(send, "[Image #1]look"),
                "expected {send:?} to carry no attachment path line"
            );
        }
        // A prompt with no placeholder never takes the attachment path at all.
        assert!(!prompt_echoes_send("look\n/home/dev/diagram.png", "look"));
    }

    /// The two-line send Claude Code was observed to wrap, and its echo verbatim.
    const PASTED_MULTI_LINE_SEND: &str =
        "これって今どこまで進んでますか\nそれともこれから開始するところですか";
    const PASTED_MULTI_LINE_ECHO: &str = "\n\n<pasted_content id=\"d626\">\nこれって今どこまで進んでますか\nそれともこれから開始するところですか\n</pasted_content id=\"d626\">\n";

    /// A single-line send of 20+ characters, wrapped the same way.
    const PASTED_SINGLE_LINE_SEND: &str = "実装に着手してほしいのですが、その前に認証をこちらで済ませておかないといけない、という理解であっていますか";
    const PASTED_SINGLE_LINE_ECHO: &str = "\n\n<pasted_content id=\"d626\">\n実装に着手してほしいのですが、その前に認証をこちらで済ませておかないといけない、という理解であっていますか\n</pasted_content id=\"d626\">\n";

    #[test]
    fn a_pasted_content_echo_matches_the_send_it_wraps() {
        assert!(prompt_echoes_send(
            PASTED_MULTI_LINE_SEND,
            PASTED_MULTI_LINE_ECHO
        ));
        assert!(prompt_echoes_send(
            PASTED_SINGLE_LINE_SEND,
            PASTED_SINGLE_LINE_ECHO
        ));
        // Surrounding whitespace on the send side is ignored, as for plain text.
        assert!(prompt_echoes_send(
            &format!("  {PASTED_SINGLE_LINE_SEND}\n"),
            PASTED_SINGLE_LINE_ECHO
        ));
    }

    #[test]
    fn a_malformed_or_partial_pasted_content_echo_does_not_match() {
        let send = "please summarize the design notes above";
        for prompt in [
            // Mismatched open/close ids.
            "\n\n<pasted_content id=\"d626\">\nplease summarize the design notes above\n</pasted_content id=\"d627\">\n",
            // Non-hex id, and ids of the wrong length.
            "\n\n<pasted_content id=\"g626\">\nplease summarize the design notes above\n</pasted_content id=\"g626\">\n",
            "\n\n<pasted_content id=\"d62\">\nplease summarize the design notes above\n</pasted_content id=\"d62\">\n",
            "\n\n<pasted_content id=\"d6260\">\nplease summarize the design notes above\n</pasted_content id=\"d6260\">\n",
            // Missing closing tag.
            "\n\n<pasted_content id=\"d626\">\nplease summarize the design notes above\n",
            // A wrapped body that differs from the send.
            "\n\n<pasted_content id=\"d626\">\nplease summarize the meeting notes above\n</pasted_content id=\"d626\">\n",
            // A well-formed block followed by extra typed text.
            "\n\n<pasted_content id=\"d626\">\nplease summarize the design notes above\n</pasted_content id=\"d626\">\nand the tests",
            // Typed text in front of the block.
            "also: \n\n<pasted_content id=\"d626\">\nplease summarize the design notes above\n</pasted_content id=\"d626\">\n",
        ] {
            assert!(
                !prompt_echoes_send(send, prompt),
                "expected {prompt:?} not to echo {send:?}"
            );
        }
    }

    #[test]
    fn a_send_containing_the_tag_itself_compares_as_a_mismatch() {
        // Claude Code escapes a tag-like `<pasted_content` inside the pasted
        // body; the escape is not reversed, so the safe answer is a mismatch.
        let send = "why does <pasted_content id=\"d626\"> show up in the log output";
        let echo = "\n\n<pasted_content id=\"d626\">\nwhy does <\\pasted_content id=\"d626\"> show up in the log output\n</pasted_content id=\"d626\">\n";
        assert!(!prompt_echoes_send(send, echo));
    }

    #[test]
    fn an_attachment_send_matches_whichever_way_the_two_rewrites_nest() {
        let send = "can you describe what this diagram is showing\n/home/dev/pictures/diagram.png";
        // Placeholder in front of a sole wrapper block.
        assert!(prompt_echoes_send(
            send,
            "[Image #3]\n\n<pasted_content id=\"a0b1\">\ncan you describe what this diagram is showing\n</pasted_content id=\"a0b1\">\n"
        ));
        // Wrapper around the placeholder and the body.
        assert!(prompt_echoes_send(
            send,
            "\n\n<pasted_content id=\"a0b1\">\n[Image #3]can you describe what this diagram is showing\n</pasted_content id=\"a0b1\">\n"
        ));
        // A different body is still a mismatch in either nesting.
        assert!(!prompt_echoes_send(
            send,
            "[Image #3]\n\n<pasted_content id=\"a0b1\">\nsomething else entirely here\n</pasted_content id=\"a0b1\">\n"
        ));
        // And the attachment count still has to line up.
        assert!(!prompt_echoes_send(
            send,
            "\n\n<pasted_content id=\"a0b1\">\ncan you describe what this diagram is showing\n</pasted_content id=\"a0b1\">\n"
        ));
    }

    #[test]
    fn image_placeholder_stripping_requires_a_well_formed_number() {
        assert_eq!(strip_image_placeholder("[Image #12]rest"), Some("rest"));
        assert_eq!(strip_image_placeholder("[Image #]rest"), None);
        assert_eq!(strip_image_placeholder("[Image #1"), None);
        assert_eq!(strip_image_placeholder("[Image #1x]rest"), None);
        assert_eq!(strip_image_placeholder("no placeholder"), None);
    }
}
