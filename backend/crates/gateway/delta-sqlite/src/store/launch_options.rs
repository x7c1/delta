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
        // NULL for a row the user registered, the preset's key for one Delta
        // ships. Every row written before the column existed reads as NULL, i.e.
        // as the user's own.
        builtin_key: row.get(7)?,
    })
}

const LAUNCH_OPTION_COLS: &str =
    "id, label, name, value, default_enabled, created_at, provider, builtin_key";

/// The list order: every row Delta ships first (ascending `id`), then the
/// user's own rows newest first.
///
/// A fixed-length leading block means a shipped row's position never moves as
/// the user adds or removes their own rows. Within the user's block, newest
/// first: the option just registered is the one they are looking for.
/// `builtin_key IS NULL` sorts `0` before `1`, and negating the id inside the
/// second key reverses the user block without needing a second `ORDER BY`
/// direction.
///
/// Ascending `id` is the order the presets were first materialized, which is
/// the declared catalog's order only for a database that met the whole catalog
/// at once: a preset added in a later release takes the next id, so it lists
/// after the shipped rows already there however early it is declared. Editing
/// the catalog's order does not reorder rows that exist.
const LAUNCH_OPTION_ORDER: &str = concat!(
    "ORDER BY (builtin_key IS NULL), ",
    "CASE WHEN builtin_key IS NULL THEN -id ELSE id END"
);

impl SqliteStore {
    pub(super) async fn list_launch_options(
        &self,
    ) -> std::result::Result<Vec<LaunchOption>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {LAUNCH_OPTION_COLS} FROM launch_option {LAUNCH_OPTION_ORDER}"
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
            // A row created through the API is always the user's own; only
            // reconciliation writes a `builtin_key`.
            builtin_key: None,
        })
    }

    pub(super) async fn launch_option(
        &self,
        id: i64,
    ) -> std::result::Result<Option<LaunchOption>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
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

    pub(super) async fn upsert_builtin_launch_option(
        &self,
        builtin_key: &str,
        label: &str,
        name: &str,
        value: Option<&str>,
        provider: AgentProvider,
    ) -> std::result::Result<LaunchOption, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        // One statement, so the row is never briefly absent and its id never
        // changes: the unique index on `builtin_key` turns a re-declared preset
        // into an in-place update. `default_enabled` appears only in the INSERT
        // half — the UPDATE half deliberately leaves it alone, because it is the
        // one field on a shipped row that belongs to the user. `created_at`
        // likewise stays at the first materialization.
        conn.execute(
            "INSERT INTO launch_option
                 (label, name, value, default_enabled, created_at, provider, builtin_key)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6)
             ON CONFLICT(builtin_key) DO UPDATE SET
                 label = excluded.label,
                 name = excluded.name,
                 value = excluded.value,
                 provider = excluded.provider",
            params![label, name, value, now, provider.as_str(), builtin_key],
        )
        .map_err(Error::from)?;
        let option = conn
            .query_row(
                &format!("SELECT {LAUNCH_OPTION_COLS} FROM launch_option WHERE builtin_key = ?1"),
                params![builtin_key],
                launch_option_from_row,
            )
            .map_err(Error::from)?;
        Ok(option)
    }

    pub(super) async fn delete_builtin_launch_options_except(
        &self,
        keys: &[&str],
    ) -> std::result::Result<usize, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Only rows Delta ships are in scope; `builtin_key IS NOT NULL` is what
        // keeps the user's own rows out of reach of a catalog change. With an
        // empty catalog every shipped row goes, which is the same statement
        // without the `NOT IN` filter.
        let (sql, params): (String, Vec<&str>) = if keys.is_empty() {
            (
                "DELETE FROM launch_option WHERE builtin_key IS NOT NULL".to_owned(),
                Vec::new(),
            )
        } else {
            let placeholders = (1..=keys.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            (
                format!(
                    "DELETE FROM launch_option
                     WHERE builtin_key IS NOT NULL AND builtin_key NOT IN ({placeholders})"
                ),
                keys.to_vec(),
            )
        };
        let removed = conn
            .execute(&sql, rusqlite::params_from_iter(params))
            .map_err(Error::from)?;
        Ok(removed)
    }
}
