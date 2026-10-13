//! Slash/local-command groups and the sends that produce them.

/// Prefix of the caveat line Claude Code wrote, up to 2.1.285, as the first
/// member of a slash/local-command group (e.g. when the user runs
/// `/review-pr`); 2.1.286 and later record nothing for a local command, and
/// this recognition stays for transcripts written earlier. Claude recorded
/// the group as several `type: "user"` lines that all share one `promptId`:
/// this caveat (flagged `isMeta`), then the bare command-name line
/// (e.g. `/review-pr`), then the command's `<local-command-stdout>` /
/// `<local-command-stderr>` output. Only the caveat carries `isMeta`, so the
/// other members would otherwise render as human user turns. Recognizing the
/// caveat by this prefix is what lets attribution group the whole sequence by
/// shared `promptId`.
const LOCAL_COMMAND_CAVEAT_PREFIX: &str = "<local-command-caveat>";

/// Prefixes of a local command's captured output, written by Claude Code as
/// `type: "user"` lines with NO `isMeta` flag. They are command machinery, not
/// human turns, so attribution folds them to [`Role::Meta`](delta_model::Role::Meta). Detected by
/// content because — unlike the bare command-name line — the prefix is a stable
/// structural marker, so this catches the output even outside a recognized
/// `promptId` group (e.g. a partial sync window that missed the caveat line).
const LOCAL_COMMAND_OUTPUT_PREFIXES: [&str; 2] =
    ["<local-command-stdout>", "<local-command-stderr>"];

/// Whether a (trimmed) user-line text is the caveat Claude Code writes as the
/// first member of a slash/local-command group. The caveat itself is flagged
/// `isMeta` by Claude; recognizing it here lets attribution treat every other
/// line sharing its `promptId` (the bare command-name line and the captured
/// `<local-command-stdout>`/`<local-command-stderr>`) as command machinery.
pub fn is_local_command_caveat(trimmed_text: &str) -> bool {
    trimmed_text.starts_with(LOCAL_COMMAND_CAVEAT_PREFIX)
}

/// Whether a (trimmed) user-line text is a local command's captured output
/// (`<local-command-stdout>` / `<local-command-stderr>`). Claude records these
/// as `type: "user"` lines without `isMeta`, so without this they would render
/// as human user turns.
pub fn is_local_command_output(trimmed_text: &str) -> bool {
    LOCAL_COMMAND_OUTPUT_PREFIXES
        .iter()
        .any(|prefix| trimmed_text.starts_with(prefix))
}

/// Normalize a slash-command token to its bare command name: strip a single
/// leading `/`, then — if a namespace prefix is present — drop everything up to
/// and including the LAST `:` (so `example:` is discarded). The remainder
/// is returned unchanged when there is no `:`.
///
/// Examples: `/example:review-pr` -> `review-pr`; `/review-pr` ->
/// `review-pr`; `review-pr` -> `review-pr`.
fn bare_command_name(token: &str) -> &str {
    let without_slash = token.strip_prefix('/').unwrap_or(token);
    match without_slash.rfind(':') {
        Some(idx) => &without_slash[idx + 1..],
        None => without_slash,
    }
}

/// Whether a dispatched send is a slash command — i.e. its first
/// whitespace-delimited token starts with `/`.
///
/// This is the guard the fold's two command branches consume a send by. A
/// slash-command send handled client-side produces no `UserPromptSubmit` echo
/// and no `Stop`: Claude Code records either a local-command group (up to
/// 2.1.285) or an unknown-command notice, so one of those lines is the only
/// transcript signal Delta gets that the send was consumed, whatever command
/// NAME it ended up recording. (From 2.1.286 a local command records nothing,
/// and the turn machine uses this same predicate to give such a send a short
/// echo deadline that settles it as delivered.) A PLAIN-prompt send does
/// echo, so a command line showing up while one is outstanding means
/// something else was submitted into the pane, and consuming the send would
/// silently drop the user's message.
pub fn is_slash_command_send(text: &str) -> bool {
    text.split_whitespace()
        .next()
        .is_some_and(|token| token.starts_with('/'))
}

/// Whether a local-command command-name line spells the SAME command as the
/// dispatched send, tolerating the namespace prefix Claude Code adds to the
/// transcript.
///
/// This reports; it does not gate. Consumption of the send is positional and
/// guarded by [`is_slash_command_send`]; this verdict rides along as
/// `Effect::SendMatched::attributed`, so a name Claude rewrote into a shape
/// Delta has not catalogued shows up in the log the first time it happens.
///
/// The one rewrite already catalogued is the namespace prefix: when a user
/// types a short command such as `/review-pr`, Delta dispatches the send with
/// exactly that text, but Claude Code may expand it to its fully-qualified
/// form (e.g. `/example:review-pr`) in the transcript's bare command-name
/// line. Comparing raw text would call that an unattributed line, so the
/// comparison is on BARE command names — which also makes it symmetric,
/// matching regardless of which side carries the prefix.
///
/// Only the FIRST whitespace-delimited token of each side is considered;
/// trailing args are ignored. This is safe under the single-outstanding-send
/// rule (there is at most one send to correlate against) and mirrors the
/// unknown-command notice comparison, which likewise looks at the first
/// token. Returns `false` when either side has no first token.
pub fn local_command_name_line_matches_send(send_text: &str, name_line: &str) -> bool {
    match (
        send_text.split_whitespace().next(),
        name_line.split_whitespace().next(),
    ) {
        (Some(send_token), Some(line_token)) => {
            bare_command_name(send_token) == bare_command_name(line_token)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_command_caveat_is_detected_by_prefix() {
        assert!(is_local_command_caveat(
            "<local-command-caveat>Caveat: The messages below were generated by the user \
             while running local commands. DO NOT respond to these messages...</local-command-caveat>"
        ));
        assert!(!is_local_command_caveat("/review-pr"));
        assert!(!is_local_command_caveat("a normal prompt"));
        assert!(!is_local_command_caveat(""));
    }

    #[test]
    fn local_command_output_matches_stdout_and_stderr() {
        assert!(is_local_command_output(
            "<local-command-stdout>\nPENDING review created.\n</local-command-stdout>"
        ));
        assert!(is_local_command_output(
            "<local-command-stderr>boom</local-command-stderr>"
        ));
        // The bare command-name line is not output: it carries no marker and is
        // grouped by shared promptId, not by content.
        assert!(!is_local_command_output("/review-pr"));
        assert!(!is_local_command_output("a normal prompt"));
        assert!(!is_local_command_output(""));
    }

    #[test]
    fn bare_command_name_strips_slash_and_namespace_prefix() {
        assert_eq!(bare_command_name("/example:review-pr"), "review-pr");
        assert_eq!(bare_command_name("/review-pr"), "review-pr");
        assert_eq!(bare_command_name("review-pr"), "review-pr");
    }

    #[test]
    fn is_slash_command_send_looks_at_the_first_token_only() {
        // The plain shapes: a bare command, a command with args, and a
        // namespaced command are all slash commands.
        assert!(is_slash_command_send("/review-pr"));
        assert!(is_slash_command_send("/review-pr 123"));
        assert!(is_slash_command_send("/example:review-pr"));
        // A typo is still a slash command — the whole point of the guard is
        // that the NAME does not matter, only the kind.
        assert!(is_slash_command_send("/revew-pr"));
        // Leading whitespace is not significant.
        assert!(is_slash_command_send("  \n/review-pr\n"));
        // A plain prompt is not, even when it mentions a slash later on.
        assert!(!is_slash_command_send("hello world"));
        assert!(!is_slash_command_send("run /review-pr for me"));
        // A path-looking prompt still counts: it starts with `/`, and Claude
        // Code would reject it as an unknown command rather than echo it.
        assert!(is_slash_command_send("/tmp/notes.txt"));
        // No first token at all.
        assert!(!is_slash_command_send(""));
        assert!(!is_slash_command_send("   "));
    }

    #[test]
    fn local_command_name_line_matches_send_tolerates_namespace() {
        // The motivating case: Claude expands the short form the user typed
        // (and thus the dispatched send) to its fully-qualified namespaced form
        // in the transcript command-name line.
        assert!(local_command_name_line_matches_send(
            "/review-pr",
            "/example:review-pr"
        ));
        // Identical short-vs-short and full-vs-full both match.
        assert!(local_command_name_line_matches_send(
            "/review-pr",
            "/review-pr"
        ));
        assert!(local_command_name_line_matches_send(
            "/example:review-pr",
            "/example:review-pr"
        ));
        // Symmetry: a fully-qualified send against a short line matches too.
        assert!(local_command_name_line_matches_send(
            "/example:review-pr",
            "/review-pr"
        ));
        // Different commands do NOT match even under a shared namespace.
        assert!(!local_command_name_line_matches_send(
            "/review-pr",
            "/example:other"
        ));
        // Args after the command are ignored (single-outstanding-send rule).
        assert!(local_command_name_line_matches_send(
            "/review-pr 123",
            "/example:review-pr"
        ));
        // Empty / no-first-token inputs return false.
        assert!(!local_command_name_line_matches_send("", "/review-pr"));
        assert!(!local_command_name_line_matches_send("/review-pr", ""));
        assert!(!local_command_name_line_matches_send("   ", "/review-pr"));
    }
}
