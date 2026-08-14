//! Response for `GET /api/clone-roots`.

use delta_usecase::CloneRoot;
use serde::Serialize;
use ts_rs::TS;

/// One registered clone root: just the absolute path. The `created_at`
/// timestamp is omitted from the wire because the client only needs the path to
/// render the row and to address the DELETE endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "CloneRoot")]
pub struct WireCloneRoot {
    pub path: String,
}

impl From<CloneRoot> for WireCloneRoot {
    fn from(root: CloneRoot) -> Self {
        WireCloneRoot { path: root.path }
    }
}

/// Response for `GET /api/clone-roots`: the registered clone roots, newest
/// first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "CloneRootsResponse")]
pub struct WireCloneRootsResponse {
    pub clone_roots: Vec<WireCloneRoot>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_response_serialises_as_an_empty_array() {
        assert_eq!(
            serde_json::to_value(WireCloneRootsResponse {
                clone_roots: Vec::new(),
            })
            .unwrap(),
            serde_json::json!({ "clone_roots": [] }),
        );
    }

    #[test]
    fn one_root_serialises_with_only_the_path_field() {
        // `created_at` is intentionally absent from the wire — the client
        // does not need it. The dropped field is also a smoke test that the
        // `From<CloneRoot>` conversion ignores it.
        let value = WireCloneRootsResponse {
            clone_roots: vec![WireCloneRoot::from(CloneRoot {
                path: "/home/dev/projects".into(),
                created_at: "2026-06-25T00:00:00Z".into(),
            })],
        };
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::json!({
                "clone_roots": [{ "path": "/home/dev/projects" }],
            }),
        );
    }
}
