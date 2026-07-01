//! Request body for `POST /api/open-cwd`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `POST /api/open-cwd`: launch an external tool (initially
/// only VS Code) at the given path.
///
/// `path` must be a path Delta has already surfaced to the browser (a
/// `session.cwd`, `session.requested_workdir`, or `message.cwd`). The server
/// checks the request path against that allowlist and rejects anything else,
/// so a hand-crafted request cannot point the editor at an arbitrary
/// directory on disk.
///
/// `handler` selects which tool to launch. It is optional; when omitted the
/// default handler (VS Code) is used. Only `"vscode"` is registered
/// initially. Sending an unregistered handler id is a `400`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "OpenCwdRequest")]
pub struct WireOpenCwdRequest {
    /// The absolute path to open. Required.
    pub path: String,
    /// Optional handler id (`"vscode"` today). Defaults to the sole
    /// registered handler when absent.
    #[serde(default)]
    pub handler: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_with_only_path() {
        let req: WireOpenCwdRequest =
            serde_json::from_str(r#"{"path":"/projects/known"}"#).unwrap();
        assert_eq!(req.path, "/projects/known");
        assert_eq!(req.handler, None);
    }

    #[test]
    fn deserializes_with_explicit_handler() {
        let req: WireOpenCwdRequest = serde_json::from_str(
            r#"{"path":"/projects/known","handler":"vscode"}"#,
        )
        .unwrap();
        assert_eq!(req.path, "/projects/known");
        assert_eq!(req.handler.as_deref(), Some("vscode"));
    }
}
