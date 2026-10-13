//! The marker Claude Code writes when the user interrupts the in-flight turn.

/// Prefix Claude Code writes to the transcript when the user interrupts the
/// in-flight turn. It appears as a `role: user` line whose only text block is
/// either `[Request interrupted by user]` (plain mid-response interrupt) or
/// `[Request interrupted by user for tool use]` (interrupt during a tool use).
/// Matching on the shared prefix covers both variants (and any future suffix)
/// without enumerating each exact string.
const INTERRUPT_MARKER_PREFIX: &str = "[Request interrupted by user";

/// Whether a (trimmed) user-line text is the interrupt marker Claude Code
/// writes when the user aborts the in-flight turn.
pub fn is_interrupt_marker(trimmed_text: &str) -> bool {
    trimmed_text.starts_with(INTERRUPT_MARKER_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_interrupt_marker_variants_match_by_prefix() {
        assert!(is_interrupt_marker("[Request interrupted by user]"));
        assert!(is_interrupt_marker(
            "[Request interrupted by user for tool use]"
        ));
        assert!(!is_interrupt_marker("a normal prompt"));
        assert!(!is_interrupt_marker(""));
    }
}
