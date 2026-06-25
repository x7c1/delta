//! Request body for `POST /api/repository-scan-roots`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `POST /api/repository-scan-roots`.
///
/// Registers one parent directory as a repository scan root: every
/// `GET /api/repositories` will probe its direct children for git clones.
/// `path` is the absolute path of the parent — not a clone itself. The server
/// trims trailing slashes for canonicalisation but does NOT require the path
/// to exist or to contain git repos at registration time, so a future-state
/// scan root is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "CreateRepositoryScanRootRequest")]
pub struct WireCreateRepositoryScanRootRequest {
    /// Absolute path of the parent directory. Required and must start with
    /// `/`; a blank or relative path is a `400`.
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_is_required_and_round_trips() {
        let req: WireCreateRepositoryScanRootRequest =
            serde_json::from_str(r#"{"path":"/home/dev/projects"}"#).unwrap();
        assert_eq!(req.path, "/home/dev/projects");
    }
}
