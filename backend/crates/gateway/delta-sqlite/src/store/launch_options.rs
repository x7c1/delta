//! The launch-option registry backing the settings screen.

use rusqlite::{params, OptionalExtension, Row};

use delta_model::{AgentProvider, LaunchOption};

use crate::error::Error;
use crate::time::now_iso8601;

use super::SqliteStore;

/// Map a `launch_option` row, in `LAUNCH_OPTION_COLS` order. Every column maps
/// directly to its domain field (no fallible status/enum parse), so this mirrors
/// [`map_session`] and returns the raw `rusqlite::Result`.
fn launch_option_from_row(row: &Row<'_>) -> rusqlite::Result<LaunchOption> {
    // `provider` is a persisted enum token (`'claude'`/`'codex'`); parse it into
    // the domain variant, surfacing an unknown token as a column-conversion
    // failure (the same shape rusqlite raises for a bad column read) rather than
    // silently defaulting. Legacy rows carry `'claude'` from the column default,
    // so they map to `AgentProvider::Claude`.
    let provider_token: String = row.get(6)?;
    let provider = AgentProvider::parse(&provider_token).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(LaunchOption {
        id: row.get(0)?,
        label: row.get(1)?,
        name: row.get(2)?,
        value: row.get(3)?,
        // SQLite stores the bool as INTEGER 0/1; `rusqlite` maps it back to `bool`.
        default_enabled: row.get(4)?,
        created_at: row.get(5)?,
        provider,
    })
}

const LAUNCH_OPTION_COLS: &str = "id, label, name, value, default_enabled, created_at, provider";

impl SqliteStore {
    pub(super) async fn list_launch_options(
        &self,
    ) -> std::result::Result<Vec<LaunchOption>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Newest first: the most recently registered option is the one a user is
        // most likely to be looking for in the settings list.
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {LAUNCH_OPTION_COLS} FROM launch_option ORDER BY id DESC"
            ))
            .map_err(Error::from)?;
        let rows = stmt
            .query_map([], launch_option_from_row)
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)?);
        }
        Ok(out)
    }

    pub(super) async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
    ) -> std::result::Result<LaunchOption, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO launch_option (label, name, value, default_enabled, created_at, provider)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![label, name, value, default_enabled, now, provider.as_str()],
        )
        .map_err(Error::from)?;
        let id = conn.last_insert_rowid();
        Ok(LaunchOption {
            id,
            label: label.map(str::to_owned),
            name: name.to_owned(),
            value: value.map(str::to_owned),
            default_enabled,
            created_at: now,
            provider,
        })
    }

    pub(super) async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> std::result::Result<Option<LaunchOption>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "UPDATE launch_option SET default_enabled = ?2 WHERE id = ?1",
                params![id, default_enabled],
            )
            .map_err(Error::from)?;
        if affected == 0 {
            return Ok(None);
        }
        let option = conn
            .query_row(
                &format!("SELECT {LAUNCH_OPTION_COLS} FROM launch_option WHERE id = ?1"),
                params![id],
                launch_option_from_row,
            )
            .optional()
            .map_err(Error::from)?;
        Ok(option)
    }

    pub(super) async fn delete_launch_option(
        &self,
        id: i64,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM launch_option WHERE id = ?1", params![id])
            .map_err(Error::from)?;
        Ok(())
    }
}
