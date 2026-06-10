use delta_model::ThreadId;

/// Extend a locator-quote frame with the thread the selected passage roots.
///
/// On the first message into a fresh branch the user has selected a passage to
/// anchor it; [`frame_locator_context`] already frames that passage. This adds a
/// note binding that passage to the thread the conversation is now in, as a
/// stable `thread:N` handle, so a later return to the same thread can re-cite it
/// by id. The id is just a handle — the quote carries the meaning.
///
/// Isolated so the exact wording is easy to tune; affects only the model-facing
/// `additionalContext`, never an on-screen message or stored field.
///
/// [`frame_locator_context`]: super::frame_locator_context
pub(in crate::interactor) fn frame_branch_entry_context(
    locator_frame: &str,
    thread: ThreadId,
) -> String {
    format!(
        "{locator_frame}\nThat passage starts a separate thread (thread:{}); the user is now talking in that thread.",
        thread.value()
    )
}
