//! The notice Claude Code writes for a slash command it does not recognize.

/// Prefix of the notice Claude Code writes when the user types a slash command
/// it does not recognize (e.g. `/review-pr` when no such command exists). Claude
/// records it as a single `type: "system"` / `subtype: "informational"` line
/// whose top-level content is `Unknown command: <command>`. Unlike a KNOWN local
/// command, an unknown command fires NEITHER a `UserPromptSubmit` echo NOR a
/// `Stop` hook and writes no `user`/`assistant` line — only this warning. Delta
/// dispatched the command as a send and moved the turn machine to `AwaitingEcho`,
/// so without recognizing this notice the send wedges the single-outstanding
/// queue forever. Attribution keys on this prefix to end the degenerate turn
/// client-side, mirroring the known-local-command handling.
const UNKNOWN_COMMAND_NOTICE_PREFIX: &str = "Unknown command:";

/// Whether a (trimmed) system-line content is the unknown-command notice Claude
/// Code writes for an unrecognized slash command.
pub fn is_unknown_command_notice(trimmed_text: &str) -> bool {
    trimmed_text.starts_with(UNKNOWN_COMMAND_NOTICE_PREFIX)
}

/// The command an unknown-command notice names, e.g. `/review-pr` from
/// `Unknown command: /review-pr`. Returns `None` when the text is not an
/// unknown-command notice or names no command after the prefix.
///
/// The notice names just the command (no args), while the send Delta dispatched
/// may carry args (`/review-pr 123`), so the caller compares this token against
/// the outstanding send's FIRST whitespace-delimited token rather than against
/// the whole send text. That comparison only feeds
/// `Effect::SendMatched::attributed`; consumption of the send is positional and
/// guarded by [`is_slash_command_send`], so a `None` here (a notice naming no
/// command at all) still consumes an outstanding slash-command send, reporting
/// it unattributed.
///
/// [`is_slash_command_send`]: super::is_slash_command_send
pub fn unknown_command_from_notice(trimmed_text: &str) -> Option<&str> {
    let rest = trimmed_text
        .strip_prefix(UNKNOWN_COMMAND_NOTICE_PREFIX)?
        .trim();
    (!rest.is_empty()).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_command_notice_is_detected_by_prefix() {
        assert!(is_unknown_command_notice("Unknown command: /review-pr"));
        // A leading-token check: a human prompt merely mentioning the phrase
        // mid-line is not the notice.
        assert!(!is_unknown_command_notice(
            "why did Unknown command: appear?"
        ));
        assert!(!is_unknown_command_notice("a normal prompt"));
        assert!(!is_unknown_command_notice(""));
    }

    #[test]
    fn unknown_command_is_extracted_from_the_notice() {
        assert_eq!(
            unknown_command_from_notice("Unknown command: /review-pr"),
            Some("/review-pr")
        );
        // The token is trimmed even with irregular spacing after the colon.
        assert_eq!(
            unknown_command_from_notice("Unknown command:   /foo  "),
            Some("/foo")
        );
        // Not a notice, or a notice naming no command, yields nothing.
        assert_eq!(unknown_command_from_notice("a normal prompt"), None);
        assert_eq!(unknown_command_from_notice("Unknown command:"), None);
        assert_eq!(unknown_command_from_notice("Unknown command:   "), None);
    }
}
