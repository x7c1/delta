//! Git facts about a candidate working directory.

/// Git facts about a candidate working directory, for the detection endpoint.
///
/// `repo_root` is the repository root containing the queried path (`None` when
/// it is not inside a git repository), and `default_branch` is that
/// repository's default branch short name when known. Computed without any
/// network access (no fetch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepoInfo {
    /// The repository root containing the queried path, or `None` when the path
    /// is not inside a git repository.
    pub repo_root: Option<String>,
    /// The repository's default branch short name (e.g. `main`), or `None` when
    /// it is unset or the path is not a git repository.
    pub default_branch: Option<String>,
}
