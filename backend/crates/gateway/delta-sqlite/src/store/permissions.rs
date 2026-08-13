//! Permission requests: recording and settling pending rows.

use rusqlite::{params, OptionalExtension, Row};

use delta_model::{PermissionRequest, PermissionStatus, SessionId};

use crate::error::{Error, Result};
use crate::time::now_iso8601;

use super::SqliteStore;

fn permission_request_from_row(row: &Row<'_>) -> Result<PermissionRequest> {
    Ok(PermissionRequest {
        id: row.get(0)?,
        session_id: SessionId::from(row.get::<_, String>(1)?),
        tool_name: row.get(2)?,
        tool_input_json: row.get(3)?,
        tool_use_id: row.get(4)?,
        status: PermissionStatus::parse(&row.get::<_, String>(5)?)?,
        decision_reason: row.get(6)?,
        created_at: row.get(7)?,
        decided_at: row.get(8)?,
    })
}

impl SqliteStore {
    pub(super) async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: Option<&str>,
    ) -> std::result::Result<PermissionRequest, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO permission_request
             (session_id, tool_name, tool_input_json, tool_use_id, status, decision_reason, created_at, decided_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', NULL, ?5, NULL)",
            params![
                session_id.as_str(),
                tool_name,
                tool_input_json,
                tool_use_id,
                now
            ],
        )
        .map_err(Error::from)?;
        let id = conn.last_insert_rowid();
        Ok(PermissionRequest {
            id,
            session_id: session_id.clone(),
            tool_name: tool_name.to_owned(),
            tool_input_json: tool_input_json.to_owned(),
            tool_use_id: tool_use_id.map(str::to_owned),
            status: PermissionStatus::Pending,
            decision_reason: None,
            created_at: now,
            decided_at: None,
        })
    }

    pub(super) async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> std::result::Result<Option<PermissionRequest>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        let status = if allowed {
            PermissionStatus::Allowed
        } else {
            PermissionStatus::Denied
        };
        // Decide only a still-`pending` row, so a late decision can never flip
        // one already settled (by an earlier decision or a tool_result).
        let row = conn
            .query_row(
                "UPDATE permission_request
                 SET status = ?1, decided_at = ?2
                 WHERE id = ?3 AND status = 'pending'
                 RETURNING id, session_id, tool_name, tool_input_json, tool_use_id,
                           status, decision_reason, created_at, decided_at",
                params![status.as_str(), now, request_id],
                |r| Ok(permission_request_from_row(r)),
            )
            .optional()
            .map_err(Error::from)?;
        row.transpose().map_err(Into::into)
    }

    pub(super) async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> std::result::Result<Vec<i64>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        let status = if allowed {
            PermissionStatus::Allowed
        } else {
            PermissionStatus::Denied
        };
        // Resolve only still-`pending` rows, so a re-ingested tool_result
        // cannot flip an already-decided row. Two kinds of row settle here:
        // the `PreToolUse`-recorded row whose `tool_use_id` matches, and any
        // pending dialog row owned by the `PermissionRequest` hook
        // (`tool_use_id IS NULL`) — the dialog blocks the session, so the next
        // tool_result to arrive is the one it gated. The returned ids are the
        // rows that transitioned, one `PermissionResolved` each.
        let mut stmt = conn
            .prepare(
                "UPDATE permission_request
                 SET status = ?1, decided_at = ?2
                 WHERE session_id = ?3 AND status = 'pending'
                   AND (tool_use_id = ?4 OR tool_use_id IS NULL)
                 RETURNING id",
            )
            .map_err(Error::from)?;
        let ids = stmt
            .query_map(
                params![status.as_str(), now, session_id.as_str(), tool_use_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(Error::from)?
            .collect::<std::result::Result<Vec<i64>, _>>()
            .map_err(Error::from)?;
        Ok(ids)
    }

    pub(super) async fn deny_pending_permission_requests(
        &self,
        session_id: &SessionId,
        reason: &str,
    ) -> std::result::Result<Vec<i64>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        // Only still-`pending` rows, like every other settle path here, so a row
        // already answered keeps the disposition it was answered with.
        let mut stmt = conn
            .prepare(
                "UPDATE permission_request
                 SET status = ?1, decision_reason = ?2, decided_at = ?3
                 WHERE session_id = ?4 AND status = 'pending'
                 RETURNING id",
            )
            .map_err(Error::from)?;
        let ids = stmt
            .query_map(
                params![
                    PermissionStatus::Denied.as_str(),
                    reason,
                    now,
                    session_id.as_str()
                ],
                |row| row.get::<_, i64>(0),
            )
            .map_err(Error::from)?
            .collect::<std::result::Result<Vec<i64>, _>>()
            .map_err(Error::from)?;
        Ok(ids)
    }
}
