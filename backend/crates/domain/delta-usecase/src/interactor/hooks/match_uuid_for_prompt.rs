use delta_model::{Message, MessageUuid};

/// Find the transcript uuid for the user line carrying this prompt.
pub(in crate::interactor::hooks) fn match_uuid_for_prompt(
    messages: &[Message],
    prompt: &str,
) -> Option<MessageUuid> {
    messages
        .iter()
        .rev()
        .find(|m| {
            matches!(m.role, delta_model::Role::User)
                && m.content_text.as_deref().map(str::trim) == Some(prompt.trim())
        })
        .map(|m| m.uuid.clone())
}
