//! The prompt-template registry backing the settings screen.

use rusqlite::{params, OptionalExtension, Row};

use delta_model::PromptTemplate;

use crate::error::Error;
use crate::time::now_iso8601;

use super::SqliteStore;

/// Map a `prompt_template` row, in `PROMPT_TEMPLATE_COLS` order. Every column
/// maps directly to its domain field (no fallible enum parse), so this returns
/// the raw `rusqlite::Result`.
fn prompt_template_from_row(row: &Row<'_>) -> rusqlite::Result<PromptTemplate> {
    Ok(PromptTemplate {
        id: row.get(0)?,
        label: row.get(1)?,
        text: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

const PROMPT_TEMPLATE_COLS: &str = "id, label, text, created_at, updated_at";

impl SqliteStore {
    pub(super) async fn list_prompt_templates(
        &self,
    ) -> std::result::Result<Vec<PromptTemplate>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Oldest first, with id as the tiebreak for rows created within the same
        // timestamp resolution. Registration order is a stable order the user
        // can build a mental map of: editing a template must not make it jump.
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {PROMPT_TEMPLATE_COLS} FROM prompt_template \
                 ORDER BY created_at ASC, id ASC"
            ))
            .map_err(Error::from)?;
        let rows = stmt
            .query_map([], prompt_template_from_row)
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)?);
        }
        Ok(out)
    }

    pub(super) async fn create_prompt_template(
        &self,
        label: &str,
        text: &str,
    ) -> std::result::Result<PromptTemplate, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO prompt_template (label, text, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)",
            params![label, text, now],
        )
        .map_err(Error::from)?;
        Ok(PromptTemplate {
            id: conn.last_insert_rowid(),
            label: label.to_owned(),
            text: text.to_owned(),
            created_at: now.clone(),
            // A never-edited template reads as updated when it was created,
            // rather than carrying a null the reader has to interpret.
            updated_at: now,
        })
    }

    pub(super) async fn update_prompt_template(
        &self,
        id: i64,
        label: &str,
        text: &str,
    ) -> std::result::Result<Option<PromptTemplate>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "UPDATE prompt_template SET label = ?2, text = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, label, text, now_iso8601()],
            )
            .map_err(Error::from)?;
        if affected == 0 {
            return Ok(None);
        }
        let template = conn
            .query_row(
                &format!("SELECT {PROMPT_TEMPLATE_COLS} FROM prompt_template WHERE id = ?1"),
                params![id],
                prompt_template_from_row,
            )
            .optional()
            .map_err(Error::from)?;
        Ok(template)
    }

    pub(super) async fn delete_prompt_template(
        &self,
        id: i64,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM prompt_template WHERE id = ?1", params![id])
            .map_err(Error::from)?;
        Ok(())
    }
}
