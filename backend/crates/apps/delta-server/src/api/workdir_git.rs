//! `/api/workdir/git` — git detection for a directory.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use delta_wire::rest::{WireGitBranchesResponse, WireGitRepoResponse};

use crate::state::AppState;

use super::ApiError;

/// Query parameters for the git-detection endpoints: the directory to inspect.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkdirGitQuery {
    /// The absolute path to inspect. Required: unlike the browse endpoints there
    /// is no sensible default repository to fall back to.
    #[serde(default)]
    path: Option<String>,
}

impl WorkdirGitQuery {
    /// The required `path` with surrounding whitespace trimmed, or a `400` when
    /// it is missing or blank. Nothing downstream trims it — the git gateway
    /// trims git's output, never the path it is handed — so the trim has to
    /// happen here for the callers to receive a usable path.
    fn require_path(&self) -> Result<&str, ApiError> {
        match self.path.as_deref().map(str::trim) {
            Some(path) if !path.is_empty() => Ok(path),
            _ => Err(ApiError::BadRequest(
                "a `path` query parameter is required".to_owned(),
            )),
        }
    }
}

/// `GET /api/workdir/git` — detect whether a directory is a git repository.
///
/// Returns `{ repo_root, default_branch }`: `repo_root` is the repository root
/// containing `path` (`null` when it is not inside a git repository), and
/// `default_branch` is that repository's default branch when known. No fetch, so
/// this is cheap to call as the picker's selection changes. A missing or blank
/// `path` is a `400`.
pub(crate) async fn workdir_git(
    State(state): State<AppState>,
    Query(query): Query<WorkdirGitQuery>,
) -> Result<Json<WireGitRepoResponse>, ApiError> {
    let path = query.require_path()?;
    let info = state.interactor().git_repo_info(path).await?;
    Ok(Json(WireGitRepoResponse::from(info)))
}

/// `GET /api/workdir/git/branches` — the remote branches of a repository.
///
/// Resolves the repository containing `path`, fetches the remote, and returns
/// `{ default_branch, remote_branches }` so a branch picker can offer a base for
/// a worktree. A `path` that is not inside a git repository is a `400` (the
/// `not a git repository` use-case error), and a missing or blank `path` is also
/// a `400`.
pub(crate) async fn workdir_git_branches(
    State(state): State<AppState>,
    Query(query): Query<WorkdirGitQuery>,
) -> Result<Json<WireGitBranchesResponse>, ApiError> {
    let path = query.require_path()?;
    let remote = state.interactor().git_remote_branches(path).await?;
    Ok(Json(WireGitBranchesResponse::from(remote)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `require_path` hands its callers a trimmed path. Asserting on the
    /// returned slice pins that directly, without a gateway stub whose only job
    /// would be to record the string it received.
    #[test]
    fn require_path_returns_a_trimmed_path() {
        let query = WorkdirGitQuery {
            path: Some("  /projects/app  ".to_owned()),
        };
        assert_eq!(query.require_path().ok(), Some("/projects/app"));
    }

    /// Missing, empty, and whitespace-only `path` values are all "blank" as far
    /// as the documented contract is concerned.
    #[test]
    fn require_path_rejects_a_blank_path() {
        for path in [None, Some(String::new()), Some("   ".to_owned())] {
            let query = WorkdirGitQuery { path: path.clone() };
            assert!(
                matches!(query.require_path(), Err(ApiError::BadRequest(_))),
                "expected a 400 for {path:?}",
            );
        }
    }
}
