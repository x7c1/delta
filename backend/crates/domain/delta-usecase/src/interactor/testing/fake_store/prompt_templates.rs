//! Prompt-template rows.

use delta_model::PromptTemplate;

use crate::error::Result;

use super::{FakeStore, FAKE_CREATED_AT, FAKE_UPDATED_AT};

impl FakeStore {
    pub(super) async fn list_prompt_templates(&self) -> Result<Vec<PromptTemplate>> {
        let g = self.inner.lock().unwrap();
        // Oldest first (ascending created_at, then id), mirroring the SQL store.
        let mut out = g.prompt_templates.clone();
        out.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(out)
    }

    pub(super) async fn create_prompt_template(
        &self,
        label: &str,
        text: &str,
    ) -> Result<PromptTemplate> {
        let mut g = self.inner.lock().unwrap();
        g.next_prompt_template_id += 1;
        let template = PromptTemplate {
            id: g.next_prompt_template_id,
            label: label.to_owned(),
            text: text.to_owned(),
            created_at: FAKE_CREATED_AT.to_owned(),
            updated_at: FAKE_CREATED_AT.to_owned(),
        };
        g.prompt_templates.push(template.clone());
        Ok(template)
    }

    pub(super) async fn update_prompt_template(
        &self,
        id: i64,
        label: &str,
        text: &str,
    ) -> Result<Option<PromptTemplate>> {
        let mut g = self.inner.lock().unwrap();
        match g.prompt_templates.iter_mut().find(|t| t.id == id) {
            Some(template) => {
                template.label = label.to_owned();
                template.text = text.to_owned();
                template.updated_at = FAKE_UPDATED_AT.to_owned();
                Ok(Some(template.clone()))
            }
            None => Ok(None),
        }
    }

    pub(super) async fn delete_prompt_template(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.prompt_templates.retain(|t| t.id != id);
        Ok(())
    }
}
