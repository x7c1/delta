use delta_model::ThreadId;

use super::delimit_quote;

/// Frame a switch back to an existing thread for injection as
/// `additionalContext`.
///
/// Delta's threads are invisible to the model, which sees only the linear
/// transcript. When the user moves to a different existing thread and continues
/// without selecting a new passage, this note tells the model the topic changed:
/// the continuation belongs to the named earlier thread, NOT the message
/// immediately above. The target thread's root quote (`root_quote`) is re-cited
/// so the re-focus holds even if the original binding scrolled out of context.
///
/// `prev` is the thread the user was just in; naming both endpoints makes the
/// move explicit. A switch is only asserted when the previous thread is known
/// and differs from the current one, so `prev` is always a concrete thread
/// here. The trunk thread (`main`) has no root quote, so `root_quote` is `None`
/// there and it is referred to by name only.
///
/// Isolated so the exact wording is easy to tune; affects only the model-facing
/// `additionalContext`, never an on-screen message or stored field.
pub(in crate::interactor) fn frame_thread_switch_context(
    prev: ThreadId,
    cur: ThreadId,
    root_quote: Option<&str>,
) -> String {
    let mut note = format!(
        "The user has switched from thread:{} to thread:{}",
        prev.value(),
        cur.value()
    );
    match root_quote {
        Some(quote) if !quote.trim().is_empty() => note.push_str(&format!(
            ", the thread rooted at this passage:\n{}",
            delimit_quote(quote)
        )),
        // `main` (or a thread with no root passage): refer to it by name only.
        _ => note.push_str(" (the main thread)"),
    }
    note.push_str(
        ".\nThey are continuing that earlier discussion, not replying to the message immediately above.",
    );
    note
}
