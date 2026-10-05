//! Response for `GET /api/storage`.

use serde::Serialize;
use ts_rs::TS;

use super::WireStorageFile;

/// Response for `GET /api/storage`: where this running Delta keeps its files,
/// and how large the ones it measures are.
///
/// Every path is absolute. `version` is the same string `GET /api/version`
/// returns. `tmux_socket` is the socket *name* passed to `tmux -L`, not a
/// path. `session_settings` is the settings file for the port this server
/// listens on. `worktree_base` and `transcript_root` sit outside `data_dir` on
/// purpose (see `delta_bootstrap::Config`).
///
/// Deliberately carries no secret: neither the hook secret nor the auth token
/// is part of this shape, even though both live beside these paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "StorageResponse")]
pub struct WireStorageResponse {
    pub identifier: String,
    pub version: String,
    pub data_dir: String,
    /// `delta.db`, with `bytes` summing `delta.db`, `delta.db-wal` and
    /// `delta.db-shm`.
    pub database: WireStorageFile,
    /// The migration runner's `delta.db.bak-v<N>` snapshots beside the
    /// database, in ascending `<N>`; empty when there are none.
    pub snapshots: Vec<WireStorageFile>,
    pub hook_state: String,
    pub sessions_dir: String,
    pub session_settings: String,
    pub tmux_conf: String,
    pub tmux_socket: String,
    pub worktree_base: String,
    pub transcript_root: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> WireStorageResponse {
        let file = |path: &str, bytes| WireStorageFile {
            path: path.to_owned(),
            bytes,
        };
        WireStorageResponse {
            identifier: "io.github.x7c1.delta".to_owned(),
            version: "v0.5.0".to_owned(),
            data_dir: "/d".to_owned(),
            database: file("/d/delta.db", 4096),
            snapshots: vec![file("/d/delta.db.bak-v3", 1024)],
            hook_state: "/d/delta-hook-state.json".to_owned(),
            sessions_dir: "/d/sessions".to_owned(),
            session_settings: "/d/settings/7878.json".to_owned(),
            tmux_conf: "/d/tmux.conf".to_owned(),
            tmux_socket: "io.github.x7c1.delta".to_owned(),
            worktree_base: "/w".to_owned(),
            transcript_root: "/t".to_owned(),
        }
    }

    /// Collect every object key in `value`, at any depth.
    fn keys(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    out.push(key.clone());
                    keys(child, out);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|item| keys(item, out)),
            _ => {}
        }
    }

    #[test]
    fn a_storage_response_carries_no_secret() {
        let mut found = Vec::new();
        keys(&serde_json::to_value(sample()).unwrap(), &mut found);
        for forbidden in ["hook_secret", "secret", "auth_token"] {
            assert!(
                !found.iter().any(|key| key == forbidden),
                "`{forbidden}` must never be serialised, got keys {found:?}",
            );
        }
    }

    #[test]
    fn a_storage_response_serializes_with_the_rest_field_names() {
        let value = serde_json::to_value(sample()).unwrap();
        assert_eq!(
            value["database"],
            serde_json::json!({ "path": "/d/delta.db", "bytes": 4096 }),
        );
        assert_eq!(value["snapshots"][0]["path"], "/d/delta.db.bak-v3");
        assert_eq!(value["tmux_socket"], "io.github.x7c1.delta");
    }
}
