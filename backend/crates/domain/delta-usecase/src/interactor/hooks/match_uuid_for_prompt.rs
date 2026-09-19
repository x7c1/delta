use delta_attribution::claude_format;
use delta_model::{Message, MessageUuid};

/// Find the transcript uuid for the user line carrying this prompt.
///
/// A human line is stored with Claude Code's pasted-content wrapper already
/// unwrapped (the attribution fold drops it so the pane shows what the user
/// wrote), while the hook's `prompt` still carries it. Unwrapping the prompt
/// with the same function puts both sides in the same form.
pub(in crate::interactor::hooks) fn match_uuid_for_prompt(
    messages: &[Message],
    prompt: &str,
) -> Option<MessageUuid> {
    let prompt = claude_format::unwrap_pasted_content(prompt);
    let prompt = prompt.trim();
    messages
        .iter()
        .rev()
        .find(|m| {
            matches!(m.role, delta_model::Role::User)
                && m.content_text.as_deref().map(str::trim) == Some(prompt)
        })
        .map(|m| m.uuid.clone())
}
