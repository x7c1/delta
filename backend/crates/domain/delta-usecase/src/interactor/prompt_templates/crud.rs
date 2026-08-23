use delta_model::PromptTemplate;

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::validate;

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// All registered prompt templates for the settings screen and the
    /// composer's insert menu, oldest first.
    pub async fn list_prompt_templates(&self) -> Result<Vec<PromptTemplate>> {
        self.store.list_prompt_templates().await
    }

    /// Register a prompt template. Both fields are required and must be
    /// non-blank; both are stored verbatim, so a `text` that ends with a newline
    /// keeps it.
    pub async fn create_prompt_template(&self, label: &str, text: &str) -> Result<PromptTemplate> {
        validate(label, text)?;
        self.store.create_prompt_template(label, text).await
    }

    /// Replace a template's content, returning the updated row (with a
    /// re-stamped `updated_at`), or `None` if no template has that id. The same
    /// non-blank rule as the create applies: an edit cannot empty a template.
    pub async fn update_prompt_template(
        &self,
        id: i64,
        label: &str,
        text: &str,
    ) -> Result<Option<PromptTemplate>> {
        validate(label, text)?;
        self.store.update_prompt_template(id, label, text).await
    }

    /// Delete a prompt template by id. Deleting an unknown id is a no-op.
    pub async fn delete_prompt_template(&self, id: i64) -> Result<()> {
        self.store.delete_prompt_template(id).await
    }
}
