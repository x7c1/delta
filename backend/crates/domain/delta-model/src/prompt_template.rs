//! The prompt-template registry.
//!
//! A named block of instruction text the user registers once and later inserts
//! into the composer at the cursor, rather than retyping or pasting the same
//! long instructions every time ("once CI is green, merge and then update the
//! plan doc…").
//!
//! Unlike a [`LaunchOption`](crate::LaunchOption), a template is **global**: the
//! text is prose addressed to whichever agent is driving the session, not argv
//! or a session-start request field, so it carries no provider and the same
//! template is offered on a Claude session and a Codex one alike.
//!
//! Delta never interprets the text — there are no placeholders and no variable
//! expansion. What is registered is what gets inserted, byte for byte.

/// A registered prompt template (one `(label, text)` record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    pub id: i64,
    /// What the template is called in the picker. Non-blank.
    pub label: String,
    /// The text inserted into the composer, stored verbatim — leading and
    /// trailing whitespace and newlines included, since a template may
    /// deliberately end with a newline. Non-blank.
    pub text: String,
    /// ISO-8601 timestamp of registration.
    pub created_at: String,
    /// ISO-8601 timestamp of the last content edit; equal to
    /// [`Self::created_at`] until the template is first edited.
    pub updated_at: String,
}
