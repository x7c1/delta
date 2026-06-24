//! Response for `GET /api/repository-scan-roots`.

use delta_usecase::RepositoryScanRoot;
use serde::Serialize;
use ts_rs::TS;

/// One registered repository scan root: just the absolute path. The
/// `created_at` timestamp is omitted from the wire because the client only
/// needs the path to render the row and to address the DELETE endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RepositoryScanRoot")]
pub struct WireRepositoryScanRoot {
    pub path: String,
}

impl From<RepositoryScanRoot> for WireRepositoryScanRoot {
    fn from(root: RepositoryScanRoot) -> Self {
        WireRepositoryScanRoot { path: root.path }
    }
}

/// Response for `GET /api/repository-scan-roots`: the registered scan roots,
/// newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RepositoryScanRootsResponse")]
pub struct WireRepositoryScanRootsResponse {
    pub scan_roots: Vec<WireRepositoryScanRoot>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_response_serialises_as_an_empty_array() {
        assert_eq!(
            serde_json::to_value(WireRepositoryScanRootsResponse {
                scan_roots: Vec::new(),
            })
            .unwrap(),
            serde_json::json!({ "scan_roots": [] }),
        );
    }

    #[test]
    fn one_root_serialises_with_only_the_path_field() {
        // `created_at` is intentionally absent from the wire — the client
        // does not need it. The dropped field is also a smoke test that the
        // `From<RepositoryScanRoot>` conversion ignores it.
        let value = WireRepositoryScanRootsResponse {
            scan_roots: vec![WireRepositoryScanRoot::from(RepositoryScanRoot {
                path: "/home/dev/projects".into(),
                created_at: "2026-06-25T00:00:00Z".into(),
            })],
        };
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::json!({
                "scan_roots": [{ "path": "/home/dev/projects" }],
            }),
        );
    }
}
