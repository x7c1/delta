//! Where an opt-in worktree session launches: the *planned* directory (and the
//! branch it will be on), computed without touching git's working tree, and the
//! build that actually produces it.
//!
//! The two are deliberately separate. A session is accepted — row written,
//! first send recorded, REST response sent — before its worktree exists, so the
//! accept phase needs the launch directory up front (it is stored as the
//! session's `cwd`) while the build itself, which can be a `git fetch` plus a
//! full checkout of a large repository, runs afterwards on the launch task.
//! That holds for every provider: a Codex session started from a PR is accepted
//! and answered exactly as a Claude one is, and builds its worktree on the same
//! background task.
//! Planning is cheap: the new-branch start points are pure string work, and
//! only `UseRemoteBranch` consults git at all (a `git worktree list`).

use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{
    GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace, WorktreeStartPoint,
};
use crate::repository::worktree_dir_slug;
use crate::send_target::WorktreeSpec;

/// Where a requested worktree will put the session, decided before any git
/// work runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::interactor) struct PlannedLaunchDir {
    /// The directory the session will launch in — the worktree the build
    /// creates, or the existing worktree it reuses.
    pub path: String,
    /// The local branch the session will be on there: the per-session
    /// `delta-<session-id>` for the new-branch start points, or the named
    /// branch for `UseRemoteBranch`.
    pub branch: String,
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// The per-session worktree path a build would create:
    /// `<worktree_base>/<slug>-<session-id>`.
    ///
    /// The directory name embeds the repository identity so listing
    /// `$DELTA_WORKTREE_BASE` shows which clone each worktree belongs to at a
    /// glance (instead of a wall of UUID-suffixed entries). The slug is the
    /// display name with `/` rewritten to `-` and any unsafe character replaced
    /// — see [`worktree_dir_slug`]. When no display name is available (the path
    /// is somehow non-git, or it slugifies to an empty string) it falls back to
    /// the literal `delta`, so the path is never just `<base>/-<id>`.
    fn default_worktree_path(
        &self,
        session_id: &delta_model::SessionId,
        repository_display_name: Option<&str>,
    ) -> String {
        let slug = repository_display_name
            .map(worktree_dir_slug)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "delta".to_owned());
        format!("{}/{}-{}", self.worktree_base, slug, session_id.as_str())
    }

    /// Decide where a requested worktree will land — and on which branch —
    /// **without creating anything**.
    ///
    /// This is the accept phase's half of the worktree work: the answer becomes
    /// the session row's `cwd` and `branch_at_launch` before the row is
    /// written, so the REST response can go out while the build is still
    /// running. [`Self::resolve_worktree_launch_dir`] performs the same
    /// decision on the launch task and must reach the same path — the launch
    /// fails outright ([`Error::WorktreeLandedElsewhere`]) if it ever does not.
    ///
    /// For the new-branch start points (`Head`/`RemoteBranch`) the answer is
    /// pure string work: the build always cuts `delta-<session-id>` at
    /// [`Self::default_worktree_path`], so those two can never diverge. For
    /// `UseRemoteBranch` the user works on the named branch itself, and git
    /// forbids one branch in two worktrees, so the worktree already holding it
    /// is reused when one exists — which needs one cheap `git worktree list`,
    /// no fetch and no checkout. That lookup is also the only way the two halves
    /// can disagree: a second session started on the same branch while the first
    /// is still checking it out plans the default path and then finds the first
    /// session's worktree at build time.
    ///
    /// [`Error::WorktreeLandedElsewhere`]: crate::error::Error::WorktreeLandedElsewhere
    pub(in crate::interactor) async fn plan_worktree_launch_dir(
        &self,
        session_id: &delta_model::SessionId,
        repo_root: &str,
        repository_display_name: Option<&str>,
        spec: &WorktreeSpec,
    ) -> Result<PlannedLaunchDir> {
        // Reject a flag-shaped or malformed remote branch name here, at the
        // accept-phase funnel every worktree start point passes through, so it
        // never reaches the `git` subprocess in the gateway (`RemoteBranch` /
        // `UseRemoteBranch` names flow to `git fetch` / `git worktree add` as
        // positional arguments). This runs before the `UseRemoteBranch` branch's
        // `git worktree list`, so a bad name spawns no git at all.
        match &spec.start_point {
            WorktreeStartPoint::Head => {}
            WorktreeStartPoint::RemoteBranch(name) | WorktreeStartPoint::UseRemoteBranch(name) => {
                check_ref_name(name)?;
            }
        }

        let default_path = self.default_worktree_path(session_id, repository_display_name);
        let planned = match &spec.start_point {
            WorktreeStartPoint::Head | WorktreeStartPoint::RemoteBranch(_) => PlannedLaunchDir {
                path: default_path,
                branch: format!("delta-{}", session_id.as_str()),
            },
            WorktreeStartPoint::UseRemoteBranch(name) => PlannedLaunchDir {
                path: self
                    .git_worktree
                    .worktree_path_for_branch(repo_root, name)
                    .await?
                    .unwrap_or(default_path),
                branch: name.clone(),
            },
        };
        Ok(planned)
    }

    /// Build (or reuse) the git worktree for an opt-in worktree request and
    /// return its path — the effective launch directory. It runs once per
    /// launch, on the shared launch task ([`spawn_launch_preparation`]), in
    /// front of whichever provider tail follows — so a session started from a
    /// PR (which always arrives as a [`WorktreeStartPoint::UseRemoteBranch`]
    /// request) lands in the same worktree regardless of the chosen provider.
    ///
    /// `repo_root` is the repository containing the user-selected workdir — the
    /// caller has already run the [`GitWorktree::repo_root`] gate — and
    /// `repository_display_name` is that repo's short identity, which shapes
    /// the directory name (see [`Self::default_worktree_path`]).
    ///
    /// For the new-branch start points (`Head`/`RemoteBranch`) a per-session
    /// `delta-<session-id>` branch is cut at `<base>/<slug>-<session-id>` —
    /// that **branch** name is kept so the frontend's `displayBranch()`
    /// shortening continues to recognise it. For `UseRemoteBranch` the worktree
    /// already holding the named branch (incl. the main tree) is reused when
    /// one exists, otherwise a new worktree checking it out is created.
    ///
    /// This is the expensive half — a `RemoteBranch` start point fetches, and
    /// every created worktree is a full checkout. It is paid on the launch
    /// task, after the accept phase's side-effect-free validation has passed
    /// and the response has gone out, so a failure here reaches the browser as
    /// a `spawn_failed` event rather than as a response body — for every
    /// provider alike.
    ///
    /// [`spawn_launch_preparation`]: super::launch_prep::spawn_launch_preparation
    pub(in crate::interactor) async fn resolve_worktree_launch_dir(
        &self,
        session_id: &delta_model::SessionId,
        repo_root: &str,
        repository_display_name: Option<&str>,
        spec: WorktreeSpec,
    ) -> Result<String> {
        let default_path = self.default_worktree_path(session_id, repository_display_name);
        let effective_path = match spec.start_point {
            // New-branch start points: cut `delta-<id>` at `default_path`.
            start_point @ (WorktreeStartPoint::Head | WorktreeStartPoint::RemoteBranch(_)) => {
                let branch = format!("delta-{}", session_id.as_str());
                self.git_worktree
                    .create_worktree(repo_root, &default_path, &branch, start_point)
                    .await?;
                default_path
            }
            // Use the branch itself: reuse the worktree already holding it
            // (incl. the main tree) when one exists, else create one that checks
            // it out at `default_path`.
            WorktreeStartPoint::UseRemoteBranch(name) => {
                match self
                    .git_worktree
                    .worktree_path_for_branch(repo_root, &name)
                    .await?
                {
                    Some(existing) => existing,
                    None => {
                        self.git_worktree
                            .add_worktree_checkout(repo_root, &default_path, &name)
                            .await?;
                        default_path
                    }
                }
            }
        };
        Ok(effective_path)
    }
}

/// Reject a remote branch/ref short name that `git` could misparse as an option
/// or that carries characters an argument must not.
///
/// The name reaches `git fetch <remote> <name>` and `git worktree add … <name>`
/// as a positional argument with no `--` guard (git does not accept `--` for
/// those positions), so a name beginning with `-` — e.g. `--upload-pack=/tmp/x`
/// — would be parsed by git as a flag rather than a ref: argument injection,
/// even though the subprocess is spawned without a shell. Rejecting the leading
/// `-` is the point; whitespace / ASCII control chars (NUL included) are refused
/// too as neither can be a legitimate ref short name. This is deliberately not a
/// full `git check-ref-format`.
fn check_ref_name(name: &str) -> Result<()> {
    let invalid = name.is_empty()
        || name.starts_with('-')
        || name
            .chars()
            .any(|c| c.is_whitespace() || c.is_ascii_control());
    if invalid {
        return Err(Error::InvalidBranchName(name.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_ref_name_rejects_a_ref_name_beginning_with_a_dash() {
        // A leading `-` is the argument-injection vector: git would parse
        // `--upload-pack=/tmp/x` as a flag on `git fetch`/`git worktree add`.
        let err = check_ref_name("--upload-pack=/tmp/x").unwrap_err();
        assert!(
            matches!(err, Error::InvalidBranchName(_)),
            "a dash-leading ref name is rejected, got: {err}"
        );
        assert!(matches!(
            check_ref_name("-x").unwrap_err(),
            Error::InvalidBranchName(_)
        ));
    }

    #[test]
    fn check_ref_name_rejects_blank_whitespace_and_control_chars() {
        for bad in ["", "a b", "a\tb", "a\nb", "a\0b"] {
            assert!(
                matches!(check_ref_name(bad), Err(Error::InvalidBranchName(_))),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn check_ref_name_accepts_ordinary_branch_names() {
        for ok in ["main", "feature/x", "release-1.2", "user/fix_bug"] {
            assert!(check_ref_name(ok).is_ok(), "expected {ok:?} to be accepted");
        }
    }
}
