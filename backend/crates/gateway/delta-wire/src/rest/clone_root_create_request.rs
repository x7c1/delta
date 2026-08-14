//! Request body for `POST /api/clone-roots`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `POST /api/clone-roots`.
///
/// Registers one directory as a clone root — a directory where the user's git
/// clones live: every `GET /api/repositories` will probe its direct children
/// for clones. `path` is that directory, not a clone itself. The server
/// trims trailing slashes for canonicalisation but does NOT require the path
/// to exist or to contain git repos at registration time, so a future-state
/// clone root is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "CreateCloneRootRequest")]
pub struct WireCreateCloneRootRequest {
    /// Absolute path of the clone root. Required and must start with `/`; a
    /// blank or relative path is a `400`.
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_is_required_and_round_trips() {
        let req: WireCreateCloneRootRequest =
            serde_json::from_str(r#"{"path":"/home/dev/projects"}"#).unwrap();
        assert_eq!(req.path, "/home/dev/projects");
    }
}
