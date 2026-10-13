//! The prompt-template registry backing the settings screen.

use async_trait::async_trait;

use delta_model::PromptTemplate;

use super::SessionStore;
use crate::error::Result;

/// The prompt-template registry backing the settings screen.
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait PromptTemplateStore: Send + Sync {
    /// All registered prompt templates, oldest first (ascending `created_at`,
    /// then ascending id). Stable insertion order: the picker's list must not
    /// reshuffle under the user when a template is edited.
    async fn list_prompt_templates(&self) -> Result<Vec<PromptTemplate>>;

    /// Register a prompt template and return the created row. Both fields are
    /// required and stored verbatim — the caller has already rejected a blank
    /// one — with `updated_at` stamped equal to `created_at`.
    async fn create_prompt_template(&self, label: &str, text: &str) -> Result<PromptTemplate>;

    /// Replace a prompt template's content, re-stamping `updated_at`, and return
    /// the updated row — or `None` if no template has that id. The id and
    /// `created_at` are preserved (a delete+recreate would churn both).
    async fn update_prompt_template(
        &self,
        id: i64,
        label: &str,
        text: &str,
    ) -> Result<Option<PromptTemplate>>;

    /// Delete a prompt template by id. Deleting an unknown id is a no-op.
    async fn delete_prompt_template(&self, id: i64) -> Result<()>;
}

#[async_trait]
impl PromptTemplateStore for Box<dyn SessionStore> {
    async fn list_prompt_templates(&self) -> Result<Vec<PromptTemplate>> {
        (**self).list_prompt_templates().await
    }

    async fn create_prompt_template(&self, label: &str, text: &str) -> Result<PromptTemplate> {
        (**self).create_prompt_template(label, text).await
    }

    async fn update_prompt_template(
        &self,
        id: i64,
        label: &str,
        text: &str,
    ) -> Result<Option<PromptTemplate>> {
        (**self).update_prompt_template(id, label, text).await
    }

    async fn delete_prompt_template(&self, id: i64) -> Result<()> {
        (**self).delete_prompt_template(id).await
    }
}
