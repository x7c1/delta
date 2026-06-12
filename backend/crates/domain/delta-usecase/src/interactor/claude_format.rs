//! Claude Code text-format detection, in one place.
//!
//! These are the textual conventions Claude Code uses on the wire Delta
//! observes (the JSONL transcript and the hook payloads), gathered here so the
//! transcript→input conversion is the only layer that knows them. The
//! structural flags (e.g. `is_queued_command`) are already detected by the
//! transcript parser in the gateway; these cover the conventions that are
//! plain strings.

/// Prefix Claude Code writes to the transcript when the user interrupts the
/// in-flight turn. It appears as a `role: user` line whose only text block is
/// either `[Request interrupted by user]` (plain mid-response interrupt) or
/// `[Request interrupted by user for tool use]` (interrupt during a tool use).
/// Matching on the shared prefix covers both variants (and any future suffix)
/// without enumerating each exact string.
const INTERRUPT_MARKER_PREFIX: &str = "[Request interrupted by user";

/// Prompt prefix Claude Code uses when it injects a background-task
/// completion notification. Such a submission is a harness injection, not a
/// human typing into the pane, so it must not be reported as external input.
const TASK_NOTIFICATION_PREFIX: &str = "<task-notification>";

/// Whether a (trimmed) user-line text is the interrupt marker Claude Code
/// writes when the user aborts the in-flight turn.
pub(in crate::interactor) fn is_interrupt_marker(trimmed_text: &str) -> bool {
    trimmed_text.starts_with(INTERRUPT_MARKER_PREFIX)
}

/// Whether a hook-submitted prompt is a harness-injected task notification
/// rather than something typed into the pane.
pub(in crate::interactor) fn is_task_notification(prompt: &str) -> bool {
    prompt.trim_start().starts_with(TASK_NOTIFICATION_PREFIX)
}
