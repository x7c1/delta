//! Outstanding subagent launches keyed by their launching tool_use_id.

use std::collections::BTreeMap;

use rusqlite::params;

use delta_attribution::SubagentLaunch;
use delta_model::{SessionId, ThreadId};
use delta_usecase::RunningSubagentLaunch;

use crate::error::Error;
use crate::time::now_iso8601;

use super::SqliteStore;

impl SqliteStore {
    pub(super) async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        // Idempotent: re-seeing the same launch (e.g. a re-ingested batch)
        // refreshes the launching thread. The `task_id` column is deliberately
        // NOT touched here — it is learned later via `upgrade_subagent_task_id`
        // and a re-record must not clobber an already-upgraded row back to NULL.
        conn.execute(
            "INSERT INTO subagent_launch (session_id, tool_use_id, thread_id, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id, tool_use_id) DO UPDATE SET
               thread_id = excluded.thread_id",
            params![session_id.as_str(), tool_use_id, thread_id.value(), now],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn upgrade_subagent_task_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        task_id: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Targeted UPDATE rather than UPSERT: the launch row must already exist
        // (the `PreToolUse(Agent)` recorded it before this hook runs). Updating
        // an unknown id silently does nothing, which is the correct no-op the
        // trait contract promises — the launch may have been folded already.
        conn.execute(
            "UPDATE subagent_launch SET task_id = ?3
             WHERE session_id = ?1 AND tool_use_id = ?2",
            params![session_id.as_str(), tool_use_id, task_id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn clear_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM subagent_launch WHERE session_id = ?1 AND tool_use_id = ?2",
            params![session_id.as_str(), tool_use_id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<BTreeMap<String, SubagentLaunch>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT tool_use_id, thread_id, task_id \
                 FROM subagent_launch WHERE session_id = ?1",
            )
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(params![session_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    SubagentLaunch {
                        thread_id: ThreadId(row.get::<_, i64>(1)?),
                        task_id: row.get::<_, Option<String>>(2)?,
                    },
                ))
            })
            .map_err(Error::from)?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (tool_use_id, launch) = row.map_err(Error::from)?;
            out.insert(tool_use_id, launch);
        }
        Ok(out)
    }

    pub(super) async fn record_subagent_indicator(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        subagent_type: Option<&str>,
        description: Option<&str>,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Targeted UPDATE, like `upgrade_subagent_task_id`: the fold records
        // the launch row before it lights the indicator, so an unknown id means
        // there is no launch to mark (a foreground call leaves none) — a no-op.
        conn.execute(
            "UPDATE subagent_launch
               SET drives_indicator = 1, subagent_type = ?3, description = ?4
             WHERE session_id = ?1 AND tool_use_id = ?2",
            params![session_id.as_str(), tool_use_id, subagent_type, description],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn running_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Vec<RunningSubagentLaunch>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Oldest first, the order the indicator lit them in; `rowid` breaks a
        // tie between launches recorded within the same timestamp.
        let mut stmt = conn
            .prepare(
                "SELECT tool_use_id, thread_id, task_id, subagent_type, description \
                 FROM subagent_launch WHERE session_id = ?1 AND drives_indicator = 1 \
                 ORDER BY created_at, rowid",
            )
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(params![session_id.as_str()], |row| {
                Ok(RunningSubagentLaunch {
                    tool_use_id: row.get(0)?,
                    thread_id: ThreadId(row.get::<_, i64>(1)?),
                    task_id: row.get(2)?,
                    subagent_type: row.get(3)?,
                    description: row.get(4)?,
                })
            })
            .map_err(Error::from)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| Error::from(err).into())
    }
}
