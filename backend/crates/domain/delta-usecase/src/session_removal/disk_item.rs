//! A piece of disk state Delta created for a session.

use std::fmt;

/// One thing on disk that removing a session considered: something Delta
/// created for the session and may therefore clean up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskItem {
    /// The git worktree at this path.
    Worktree(String),
    /// The local git branch of this name.
    Branch(String),
    /// The `projects.<path>` entry Delta seeded for this path in Claude Code's
    /// user config (`~/.claude.json`).
    TrustEntry(String),
}

impl fmt::Display for DiskItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Worktree(path) => write!(f, "worktree {path}"),
            Self::Branch(name) => write!(f, "branch {name}"),
            Self::TrustEntry(path) => write!(f, "trust entry for {path}"),
        }
    }
}
