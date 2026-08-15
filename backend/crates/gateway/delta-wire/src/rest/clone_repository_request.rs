//! Request body for `POST /api/repositories/clone`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `POST /api/repositories/clone`.
///
/// Asks the server to clone `<repo_owner>/<repo_name>` into `clone_root`, which
/// must be one of the registered clone roots. The destination is exactly
/// `<clone_root>/<repo_name>` — the request never names it, because there is no
/// fallback naming to choose between: either that path is free and the clone
/// lands there, or the request is refused.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "CloneRepositoryRequest")]
pub struct WireCloneRepositoryRequest {
    /// The repository's owner (`x7c1` in `x7c1/delta`).
    pub repo_owner: String,
    /// The repository's name (`delta` in `x7c1/delta`). Also the destination
    /// directory's name inside the clone root.
    pub repo_name: String,
    /// A registered clone root, spelled exactly as `GET /api/clone-roots`
    /// returns it. An unregistered path is a `400` with code
    /// `clone_root_not_registered`.
    pub clone_root: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_body_deserializes_into_all_three_fields() {
        let req: WireCloneRepositoryRequest = serde_json::from_str(
            r#"{"repo_owner":"x7c1","repo_name":"delta","clone_root":"/home/dev/projects"}"#,
        )
        .unwrap();
        assert_eq!(req.repo_owner, "x7c1");
        assert_eq!(req.repo_name, "delta");
        assert_eq!(req.clone_root, "/home/dev/projects");
    }

    #[test]
    fn a_body_missing_the_clone_root_is_rejected() {
        // The clone root is not optional: there is no "pick one for me" default,
        // since which root a clone belongs in is the user's decision.
        let parsed: Result<WireCloneRepositoryRequest, _> =
            serde_json::from_str(r#"{"repo_owner":"x7c1","repo_name":"delta"}"#);
        assert!(parsed.is_err());
    }
}
