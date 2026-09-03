//! Session lifecycle use cases: spawn, resume (open), close, and the
//! launch-readiness ticks. The `ensure`/`new` entry points live on the
//! interactor's routing layer (they mint the session id and pick the actor);
//! everything here runs inside a session's actor.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

mod adapter_launch;
mod adapter_session;
mod cancel_launch;
mod close_session;
mod dispatch_ready_resumes;
mod finish_launch;
mod launch_prep;
mod mint_free_token;
mod open_session;
mod reap_stale_spawns;
mod record_launched_pane;
mod spawn_fresh;
mod workdir_for;
mod worktree_launch_dir;

pub(in crate::interactor) use adapter_launch::PreparedAdapterLaunch;
pub(in crate::interactor) use cancel_launch::UnboundLaunchEnd;
pub(in crate::interactor) use record_launched_pane::LaunchApproval;
pub(in crate::interactor) use spawn_fresh::FreshSpawn;

#[cfg(test)]
mod tests;

/// The `--resume` flag passed to `claude` to reattach to a stored conversation.
const RESUME_FLAG: &str = "--resume";

/// The `--session-id` flag passed to `claude` to pin a fresh conversation's
/// `session_id` to a value Delta mints up front. With the id known at spawn
/// time, the first `UserPromptSubmit` hook reports exactly that id, so a fresh
/// spawn correlates to its session by id — never by working directory.
const SESSION_ID_FLAG: &str = "--session-id";

/// The `--settings` flag passed to `claude` to load Delta's session settings
/// (hooks + theme) from a Delta-owned file, instead of writing them into the
/// session's working directory and risking a clobber of a real project's
/// `.claude/settings.json`.
const SETTINGS_FLAG: &str = "--settings";

/// True iff `candidate` resolves to a path at or below `base` — the sole gate on
/// pre-accepting Claude Code's workspace-trust dialog.
///
/// The trade-off this defends: [`ensure_dir_trusted`] writes
/// `hasTrustDialogAccepted` into the user's GLOBAL `~/.claude.json`, which also
/// silences the dialog in the user's own plain `claude` sessions in that
/// directory — so any checked-in `.claude/settings.json` hooks there would then
/// run unprompted. Delta therefore auto-accepts ONLY directories it itself
/// created under its own worktree base; any other directory (a repo the user
/// pointed Delta at, a clone, a worktree made elsewhere) gets Claude Code's
/// normal one-time trust dialog instead.
///
/// Robustness: both paths are canonicalized (their longest existing ancestor is
/// resolved, so `/tmp` vs `/private/tmp` on macOS and any `..` collapse) and then
/// compared by path COMPONENTS via [`Path::starts_with`], never by a string
/// prefix — otherwise a sibling like `<base>-evil` would read as "under"
/// `<base>`. A candidate whose directory does not exist yet (a worktree is
/// trust-checked before it is built) still resolves through its existing
/// ancestor.
///
/// [`ensure_dir_trusted`]: crate::ports::GitWorktree::ensure_dir_trusted
pub(in crate::interactor) fn is_under_worktree_base(base: &str, candidate: &str) -> bool {
    let base = canonicalize_existing_ancestor(Path::new(base));
    let candidate = canonicalize_existing_ancestor(Path::new(candidate));
    candidate.starts_with(&base)
}

/// Canonicalize `path` by resolving the longest ancestor that exists on disk and
/// re-appending the remaining (not-yet-created) components verbatim.
///
/// [`std::fs::canonicalize`] requires the *whole* path to exist, but a Delta
/// worktree is trust-checked before `git worktree add` builds it. Resolving the
/// existing prefix is what defeats the `/tmp`↔`/private/tmp` symlink and any
/// `..` inside it; the not-yet-created tail is kept as-is. A path with no
/// existing ancestor (or one whose tail ends in `..`, which has no file name to
/// peel off) falls back to the path as given rather than claiming a match.
fn canonicalize_existing_ancestor(path: &Path) -> PathBuf {
    let mut ancestor = path;
    let mut tail: Vec<OsString> = Vec::new();
    loop {
        if let Ok(canonical) = ancestor.canonicalize() {
            let mut resolved = canonical;
            resolved.extend(tail.iter().rev());
            return resolved;
        }
        match (ancestor.file_name(), ancestor.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_owned());
                ancestor = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}
