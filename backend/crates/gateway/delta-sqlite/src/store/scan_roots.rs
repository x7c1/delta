//! Repository scan roots configured in settings.

use rusqlite::params;

use delta_usecase::RepositoryScanRoot;

use crate::error::Error;
use crate::time::now_iso8601;

use super::SqliteStore;

impl SqliteStore {
    pub(super) async fn list_repository_scan_roots(
        &self,
    ) -> std::result::Result<Vec<RepositoryScanRoot>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Newest first: the most recently added scan root is the one a user is
        // most likely to be looking for in the Settings list (mirroring
        // `list_launch_options`).
        let mut stmt = conn
            .prepare(
                "SELECT path, created_at FROM repository_scan_root \
                 ORDER BY created_at DESC, path ASC",
            )
            .map_err(Error::from)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RepositoryScanRoot {
                    path: row.get::<_, String>(0)?,
                    created_at: row.get::<_, String>(1)?,
                })
            })
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)?);
        }
        Ok(out)
    }

    pub(super) async fn insert_repository_scan_root(
        &self,
        path: &str,
    ) -> std::result::Result<RepositoryScanRoot, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        let inserted = conn.execute(
            "INSERT INTO repository_scan_root (path, created_at) VALUES (?1, ?2)",
            params![path, now],
        );
        match inserted {
            Ok(_) => Ok(RepositoryScanRoot {
                path: path.to_owned(),
                created_at: now,
            }),
            // The PRIMARY KEY constraint is the conflict gate: surface duplicates
            // as a typed use-case error so the HTTP layer can map them to 409
            // without parsing the generic store-error string.
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(delta_usecase::Error::RepositoryScanRootDuplicate(
                    path.to_owned(),
                ))
            }
            Err(err) => Err(Error::from(err).into()),
        }
    }

    pub(super) async fn delete_repository_scan_root(
        &self,
        path: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Idempotent: an explicit Remove click should not 404 on a path the user
        // just removed via another tab; the row is gone either way after the call.
        conn.execute(
            "DELETE FROM repository_scan_root WHERE path = ?1",
            params![path],
        )
        .map_err(Error::from)?;
        Ok(())
    }
}
