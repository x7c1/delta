//! Where a new session's worktree starts from.

/// Where a new session's worktree should start from, and whether it gets its
/// own `delta-<id>` branch or works on an existing branch directly.
///
/// A fresh session opts into a worktree by naming a start point:
/// - `Head` / `RemoteBranch` cut a *new* `delta-<id>` branch — from the current
///   checkout, or from a named remote branch (fetched first so the worktree
///   always starts from the latest remote tip).
/// - `UseRemoteBranch` instead works on the named branch *itself* in the
///   worktree (no `delta-<id>` branch). Because git forbids checking one branch
///   out in two worktrees, the orchestration reuses the worktree that already
///   has that branch checked out (including the main working tree) when one
///   exists, and otherwise creates a worktree that checks the branch out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeStartPoint {
    /// Cut a new `delta-<id>` branch off the repository's current `HEAD`. No
    /// network access.
    Head,
    /// Cut a new `delta-<id>` branch off `origin/<name>`, fetched first so it
    /// reflects the latest remote tip. The carried name is the remote branch's
    /// short name (no `origin/` prefix), e.g. `main`.
    RemoteBranch(String),
    /// Work on the branch `<name>` itself in the worktree (no `delta-<id>`
    /// branch). The carried name is the branch's short name (no `origin/`
    /// prefix). The orchestration reuses an existing worktree that already has
    /// `<name>` checked out, or creates one that checks it out (creating a
    /// local tracking branch off `origin/<name>` first when no local branch
    /// exists yet).
    UseRemoteBranch(String),
}
