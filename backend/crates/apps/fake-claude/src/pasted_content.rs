//! Claude Code's `<pasted_content>` wrapper, reproduced on the fake's side.
//!
//! Recent Claude Code builds (2.1.277 and later, behind a server-side flag)
//! wrap text that arrived as a paste in a tag pair before submitting the
//! prompt, once the paste's trimmed length reaches 20 characters. Both the
//! `UserPromptSubmit` hook's `prompt` and the transcript's `type: "user"` line
//! carry the wrapped form. Observed on a prompt that is nothing but the paste
//! (Rust string literal):
//!
//! ```text
//! "\n\n<pasted_content id=\"d626\">\n<body>\n</pasted_content id=\"d626\">\n"
//! ```
//!
//! - the id is four lowercase hex characters, the same on both tags and
//!   stable for the session;
//! - a newline follows the opening tag, and one precedes the closing tag
//!   unless the body already ends with a newline;
//! - a tag-like `<pasted_content` inside the body is escaped to
//!   `<\pasted_content`;
//! - text typed next to the paste stays outside the block, and a paste under
//!   the threshold is not wrapped at all.
//!
//! Delta delivers every send as a bracketed paste, so a scenario that opts in
//! (`"wrap_pastes": true`) makes every long enough send come back wrapped, the
//! way it does against a real `claude` with the flag on.

use crate::input::Submission;

/// The trimmed length, in characters, from which a paste is wrapped.
pub const WRAP_THRESHOLD_CHARS: usize = 20;

/// The opening tag's name, and what the escape rewrites inside a body.
const TAG_LIKE: &str = "<pasted_content";

/// Its escaped form.
const ESCAPED_TAG_LIKE: &str = "<\\pasted_content";

/// The session's four-lowercase-hex-character block id. Claude Code keeps one
/// id for the session; the fake derives it from the session id (FNV-1a folded
/// to 16 bits) so a run is reproducible.
pub fn session_paste_id(session_id: &str) -> String {
    let hash = session_id.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    format!("{:04x}", (hash >> 16) ^ (hash & 0xffff))
}

/// `submission` as Claude Code submits it with the wrapper on: every pasted
/// part whose trimmed length reaches [`WRAP_THRESHOLD_CHARS`] becomes a block
/// with id `id`, and everything else is kept as it was entered.
pub fn wrap_submission(submission: &Submission, id: &str) -> String {
    submission
        .parts
        .iter()
        .map(|part| {
            if part.pasted && part.text.trim().chars().count() >= WRAP_THRESHOLD_CHARS {
                wrap_block(&part.text, id)
            } else {
                part.text.clone()
            }
        })
        .collect()
}

/// One pasted body as a block (see the module docs for the shape).
fn wrap_block(body: &str, id: &str) -> String {
    let body = body.replace(TAG_LIKE, ESCAPED_TAG_LIKE);
    let newline_before_close = if body.ends_with('\n') { "" } else { "\n" };
    format!(
        "\n\n<pasted_content id=\"{id}\">\n{body}{newline_before_close}</pasted_content id=\"{id}\">\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PromptPart;

    const ID: &str = "3b9c";

    /// Long enough to be wrapped.
    const LONG: &str = "a pasted passage well over twenty characters";

    fn submission(parts: &[(&str, bool)]) -> Submission {
        Submission {
            parts: parts
                .iter()
                .map(|&(text, pasted)| PromptPart {
                    text: text.to_owned(),
                    pasted,
                })
                .collect(),
        }
    }

    #[test]
    fn the_block_id_is_four_lowercase_hex_characters_and_stable() {
        for session in ["sess-1", "4f1c2b8e-0000-4000-8000-000000000000", ""] {
            let id = session_paste_id(session);
            assert_eq!(id.len(), 4, "{id:?}");
            assert!(
                id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "{id:?}"
            );
            assert_eq!(session_paste_id(session), id);
        }
        assert_ne!(session_paste_id("sess-1"), session_paste_id("sess-2"));
    }

    #[test]
    fn a_long_paste_is_wrapped_in_the_observed_shape() {
        assert_eq!(
            wrap_submission(&submission(&[(LONG, true)]), ID),
            format!("\n\n<pasted_content id=\"3b9c\">\n{LONG}\n</pasted_content id=\"3b9c\">\n")
        );
    }

    #[test]
    fn a_body_ending_in_a_newline_gets_no_second_one_before_the_closing_tag() {
        assert_eq!(
            wrap_submission(&submission(&[(&format!("{LONG}\n"), true)]), ID),
            format!("\n\n<pasted_content id=\"3b9c\">\n{LONG}\n</pasted_content id=\"3b9c\">\n")
        );
    }

    #[test]
    fn a_tag_like_string_inside_the_body_is_escaped() {
        let body = "quoting <pasted_content id=\"d626\"> in a paste";
        assert_eq!(
            wrap_submission(&submission(&[(body, true)]), ID),
            "\n\n<pasted_content id=\"3b9c\">\nquoting <\\pasted_content id=\"d626\"> in a paste\n</pasted_content id=\"3b9c\">\n"
        );
    }

    #[test]
    fn the_threshold_is_twenty_trimmed_characters() {
        let nineteen = "abcdefghijklmnopqrs";
        let twenty = "abcdefghijklmnopqrst";
        // Surrounding whitespace does not count towards the threshold.
        let padded = format!("  \n{nineteen}\n  ");
        assert_eq!(
            wrap_submission(&submission(&[(nineteen, true)]), ID),
            nineteen
        );
        assert_eq!(wrap_submission(&submission(&[(&padded, true)]), ID), padded);
        assert!(wrap_submission(&submission(&[(twenty, true)]), ID)
            .starts_with("\n\n<pasted_content id=\"3b9c\">\n"));
        // Characters, not bytes: 19 multi-byte characters stay bare.
        let wide = "あ".repeat(19);
        assert_eq!(wrap_submission(&submission(&[(&wide, true)]), ID), wide);
    }

    #[test]
    fn typed_text_is_never_wrapped() {
        assert_eq!(wrap_submission(&submission(&[(LONG, false)]), ID), LONG);
        assert_eq!(wrap_submission(&Submission::typed(LONG), ID), LONG);
    }

    #[test]
    fn typed_text_next_to_a_paste_stays_outside_the_block() {
        assert_eq!(
            wrap_submission(&submission(&[("look: ", false), (LONG, true), (" ok?", false)]), ID),
            format!(
                "look: \n\n<pasted_content id=\"3b9c\">\n{LONG}\n</pasted_content id=\"3b9c\">\n ok?"
            )
        );
    }
}
