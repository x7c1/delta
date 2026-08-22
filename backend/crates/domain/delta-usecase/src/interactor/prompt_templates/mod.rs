//! Prompt-template registry use cases: list, create, update, and delete the
//! named blocks of instruction text the user later inserts into the composer.
//!
//! Each operation is a thin pass-through to the [`SessionStore`] port — the
//! registry has no cross-record invariants — with one rule applied here rather
//! than at the transport: a template's `label` and `text` must not be blank
//! ([`validate`]). Keeping it in the use case means the HTTP handler and any
//! future caller are held to the same contract.
//!
//! [`SessionStore`]: crate::ports::SessionStore

mod crud;

#[cfg(test)]
mod tests;

use crate::error::{Error, Result};

/// Reject a template whose `label` or `text` is blank.
///
/// "Blank" is judged on the trimmed value, but **only** for this check: the
/// caller stores the originals. A template is prose destined for a composer, so
/// its own leading/trailing whitespace and newlines are content — a template
/// that ends with a newline so the next paragraph starts on its own line means
/// exactly that — while a label or body that is nothing but whitespace is
/// unpickable and inserts nothing.
pub(in crate::interactor) fn validate(label: &str, text: &str) -> Result<()> {
    if label.trim().is_empty() {
        return Err(Error::InvalidPromptTemplate(
            "a prompt template must have a non-blank `label`".to_owned(),
        ));
    }
    if text.trim().is_empty() {
        return Err(Error::InvalidPromptTemplate(
            "a prompt template must have non-blank `text`".to_owned(),
        ));
    }
    Ok(())
}
