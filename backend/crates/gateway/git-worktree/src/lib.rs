//! git-backed [`delta_usecase::GitWorktree`] implementation.
//!
//! [`Git`] detects git repositories and creates per-session git worktrees by
//! shelling out to `git`. It is stateless: every method takes the repository
//! (or candidate) path explicitly and invokes `git -C <path> …`, so one
//! instance serves any number of repositories and never depends on the
//! process's current directory. This mirrors the tmux driver, the project's
//! other subprocess gateway.

mod error;
mod git;

pub use error::{Error, Result};
pub use git::Git;
