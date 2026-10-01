//! The session row's remembered pane: the `tmux_session`, `tmux_pane` and
//! `hooks_unreachable` columns (see the `session` migration module for what
//! they mean and when they are NULL).

use rusqlite::{params, OptionalExtension, Row};

use delta_model::SessionId;
use delta_usecase::RememberedPane;

use crate::error::Error;

use super::SqliteStore;

/// The columns a remembered pane is read from, in [`remembered_pane_from_row`]
/// order starting at index `0`.
const REMEMBERED_PANE_COLUMNS: &str = "tmux_session, tmux_pane, hooks_unreachable";

/// Map the [`REMEMBERED_PANE_COLUMNS`] starting at `offset` to a pane, or
/// `None` when the row remembers none.
///
/// Both names are written and cleared together, so a row holding only one of
/// them is not something Delta writes; it reads as "nothing remembered" rather
/// than as half a pane nobody could address.
fn remembered_pane_from_row(
    row: &Row<'_>,
    offset: usize,
) -> rusqlite::Result<Option<RememberedPane>> {
    let tmux_session: Option<String> = row.get(offset)?;
    let pane: Option<String> = row.get(offset + 1)?;
    let hooks_unreachable: bool = row.get(offset + 2)?;
    Ok(match (tmux_session, pane) {
        (Some(tmux_session), Some(pane)) => Some(RememberedPane {
            tmux_session,
            pane,
            hooks_unreachable,
        }),
        _ => None,
    })
}

impl SqliteStore {
    pub(super) async fn remember_pane(
        &self,
        id: &SessionId,
        pane: &RememberedPane,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let mut conn = self.conn.lock().await;
        let tx = conn.transaction().map_err(Error::from)?;
        let written = tx
            .execute(
                "UPDATE session SET tmux_session = ?2, tmux_pane = ?3, hooks_unreachable = ?4 \
                 WHERE id = ?1",
                params![
                    id.as_str(),
                    pane.tmux_session,
                    pane.pane,
                    pane.hooks_unreachable
                ],
            )
            .map_err(Error::from)?;
        // The name now belongs to this row: any other row still naming it
        // remembers a pane that died before the name was minted again (see
        // the port's contract), so its record is cleared in the same write.
        if written > 0 {
            tx.execute(
                "UPDATE session SET tmux_session = NULL, tmux_pane = NULL, hooks_unreachable = 0 \
                 WHERE tmux_session = ?2 AND id <> ?1",
                params![id.as_str(), pane.tmux_session],
            )
            .map_err(Error::from)?;
        }
        tx.commit().map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn forget_pane(
        &self,
        id: &SessionId,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE session SET tmux_session = NULL, tmux_pane = NULL, hooks_unreachable = 0 \
             WHERE id = ?1",
            params![id.as_str()],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn remembered_pane(
        &self,
        id: &SessionId,
    ) -> std::result::Result<Option<RememberedPane>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let pane = conn
            .query_row(
                &format!("SELECT {REMEMBERED_PANE_COLUMNS} FROM session WHERE id = ?1"),
                params![id.as_str()],
                |row| remembered_pane_from_row(row, 0),
            )
            .optional()
            .map_err(Error::from)?;
        Ok(pane.flatten())
    }

    pub(super) async fn remembered_panes(
        &self,
    ) -> std::result::Result<Vec<(SessionId, RememberedPane)>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id, {REMEMBERED_PANE_COLUMNS} FROM session \
                 WHERE tmux_session IS NOT NULL AND tmux_pane IS NOT NULL \
                 ORDER BY created_at, id"
            ))
            .map_err(Error::from)?;
        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                Ok((SessionId::from(id), remembered_pane_from_row(row, 1)?))
            })
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            if let (id, Some(pane)) = row.map_err(Error::from)? {
                out.push((id, pane));
            }
        }
        Ok(out)
    }
}
