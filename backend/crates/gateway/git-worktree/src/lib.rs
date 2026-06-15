//! git-backed [`delta_usecase::GitWorktree`] implementation.
//!
//! [`Git`] detects git repositories and creates per-session git worktrees by
//! shelling out to `git`. Its git operations are stateless: every git method
//! takes the repository (or candidate) path explicitly and invokes
//! `git -C <path> …`, so one instance serves any number of repositories and
//! never depends on the process's current directory. This mirrors the tmux
//! driver, the project's other subprocess gateway.
//!
//! It also seeds Claude Code's per-directory workspace-trust flag in the user's
//! config so a programmatic launch in a fresh directory is not blocked by the
//! interactive trust dialog; see [`mod@trust`].

mod error;
mod git;
mod trust;

pub use error::{Error, Result};
pub use git::Git;
