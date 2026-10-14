//! The launch-trust rule: when Delta may pre-accept Claude Code's workspace
//! trust for a worktree it builds, and the check the new-session flow runs
//! before launching so the user can confirm what would otherwise be trusted
//! silently.
//!
//! Trust is what lets a project's checked-in agent configuration take effect
//! without a prompt, and the content of a worktree made from a remote branch
//! is decided by that branch's author, not by the user. So Delta pre-accepts
//! only where the user has already made the decision:
//!
//! - **Rule A** — the repository the worktree comes from (the one the user
//!   picked) is trusted in Claude Code's config. An untrusted repository needs
//!   confirmation; confirming it records trust for the repository root too,
//!   so the question is asked once per repository.
//! - **Rule B** — for a worktree started from a remote branch, the branch's
//!   agent configuration ([`is_agent_config_path`]) is identical to the
//!   repository `HEAD` the user trusts. Any difference needs confirmation,
//!   with the diff shown.
//!
//! The confirmation is bound to a commit: [`LaunchTrustCheck::commit`] is
//! what the user confirms, the launch carries it back
//! ([`WorktreeSpec::confirmed_commit`]), and the launch task re-applies the
//! rule to the commit the built worktree is actually on. A confirmation for
//! any other commit counts for nothing ([`Error::LaunchNeedsConfirmation`]).
//! Every comparison reads git objects; nothing in the worktree is read as a
//! file.
//!
//! [`WorktreeSpec::confirmed_commit`]: crate::WorktreeSpec::confirmed_commit

use crate::agent::AgentProvider;
use crate::error::{Error, Result};
use crate::interactor::session_actor::runtime::{LaunchingSpawn, PlannedWorktree};
use crate::interactor::InteractorCore;
use crate::launch_trust::{
    is_agent_config_path, truncate_diff, AgentConfigChange, LaunchTrustCheck, MAX_DIFFED_PATHS,
};
use crate::ports::{
    GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace, WorktreeStartPoint,
};

use super::worktree_launch_dir::check_ref_name;

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Whether launching a new `provider` session in a worktree of `workdir`
    /// started from `start_point` needs the user's confirmation, and what to
    /// show them if so.
    ///
    /// Backs `POST /api/launch-trust`, which the new-session flow calls before
    /// sending. It creates nothing — no worktree, no session — and writes
    /// nothing to Claude Code's config; for a remote start point it runs the
    /// same fetch the build would, so the commit it reports is the one the
    /// build will check out unless the branch moves in between (which the
    /// launch then catches; see the module docs).
    ///
    /// No confirmation is needed where Delta pre-accepts nothing: a provider
    /// that does not run Claude Code in a pane, or a `use_remote_branch` start
    /// point whose branch is already checked out in a worktree outside the
    /// worktree base (it is reused, and Claude Code's own dialog applies
    /// there) or in one that is already trusted.
    pub async fn check_launch_trust(
        &self,
        workdir: &str,
        start_point: &WorktreeStartPoint,
        provider: AgentProvider,
    ) -> Result<LaunchTrustCheck> {
        if self.adapter_backed_factory(provider).is_some() {
            return Ok(LaunchTrustCheck::not_applicable());
        }
        if let WorktreeStartPoint::RemoteBranch(name) | WorktreeStartPoint::UseRemoteBranch(name) =
            start_point
        {
            check_ref_name(name)?;
        }
        let dir = self.workspace.resolve_existing_dir(workdir).await?;
        let repo_root = self
            .git_worktree
            .repo_root(&dir)
            .await?
            .ok_or_else(|| Error::WorktreeNotAGitRepo(dir.clone()))?;
        if let WorktreeStartPoint::UseRemoteBranch(name) = start_point {
            if let Some(existing) = self
                .git_worktree
                .worktree_path_for_branch(&repo_root, name)
                .await?
            {
                if !super::is_under_worktree_base(&self.worktree_base, &existing)
                    || self.git_worktree.is_dir_trusted(&existing).await?
                {
                    return Ok(LaunchTrustCheck::not_applicable());
                }
            }
        }
        let commit = self
            .git_worktree
            .launch_commit(&repo_root, start_point)
            .await?;
        self.assess_launch_trust(&repo_root, start_point, commit)
            .await
    }

    /// Apply rules A and B to a worktree of `repo_root`, started from
    /// `start_point`, that is (or will be) on `commit`.
    async fn assess_launch_trust(
        &self,
        repo_root: &str,
        start_point: &WorktreeStartPoint,
        commit: String,
    ) -> Result<LaunchTrustCheck> {
        let repository_untrusted = !self.git_worktree.is_dir_trusted(repo_root).await?;
        let agent_config_changes = match start_point {
            // A new branch off the user's own `HEAD` carries exactly the
            // content the user has checked out; only rule A applies.
            WorktreeStartPoint::Head => Vec::new(),
            WorktreeStartPoint::RemoteBranch(_) | WorktreeStartPoint::UseRemoteBranch(_) => {
                self.agent_config_changes(repo_root, &commit).await?
            }
        };
        Ok(LaunchTrustCheck {
            repository_untrusted,
            commit: Some(commit),
            agent_config_changes,
        })
    }

    /// The agent-configuration paths whose content differs between
    /// `repo_root`'s `HEAD` (what the user trusts) and `commit`, each with its
    /// diff (bounded by [`MAX_DIFFED_PATHS`] and the per-diff size limit).
    async fn agent_config_changes(
        &self,
        repo_root: &str,
        commit: &str,
    ) -> Result<Vec<AgentConfigChange>> {
        let trusted = self.git_worktree.head_commit(repo_root).await?;
        let mut changes = Vec::new();
        for path in self
            .git_worktree
            .changed_paths(repo_root, &trusted, commit)
            .await?
            .into_iter()
            .filter(|path| is_agent_config_path(path))
        {
            let (diff, diff_truncated) = if changes.len() < MAX_DIFFED_PATHS {
                truncate_diff(
                    self.git_worktree
                        .file_diff(repo_root, &trusted, commit, &path)
                        .await?,
                )
            } else {
                (String::new(), true)
            };
            changes.push(AgentConfigChange {
                path,
                diff,
                diff_truncated,
            });
        }
        Ok(changes)
    }

    /// Pre-accept Claude Code's trust for a launch's directory where the
    /// launch-trust rule allows it, on the launch task, once the worktree is
    /// built and before the agent starts.
    ///
    /// Only a git working tree under Delta's worktree base is ever
    /// pre-accepted: a directory the user pointed Delta at keeps Claude Code's
    /// own dialog, since pre-accepting it would also silence the dialog in the
    /// user's plain `claude` sessions there. Within the base:
    ///
    /// - a directory already trusted (a reused worktree) only has its keys
    ///   refreshed;
    /// - a worktree this launch built is pre-accepted when rules A and B hold
    ///   for the commit it is on, or when the launch confirmed exactly that
    ///   commit — in which case an untrusted repository is recorded as trusted
    ///   too;
    /// - anything else fails the launch with [`Error::LaunchNeedsConfirmation`]
    ///   rather than starting the agent behind a dialog nobody can answer.
    pub(super) async fn pre_accept_launch_trust(&self, launching: &LaunchingSpawn) -> Result<()> {
        let workdir = &launching.workdir;
        if !super::is_under_worktree_base(&self.worktree_base, workdir) {
            return Ok(());
        }
        if self.git_worktree.is_dir_trusted(workdir).await? {
            return self.git_worktree.ensure_dir_trusted(workdir).await;
        }
        let Some(PlannedWorktree {
            repo_root, spec, ..
        }) = &launching.worktree
        else {
            // A directory this launch did not build and the user has not
            // trusted: nothing about it was confirmed, so leave it to Claude
            // Code's own dialog.
            return Ok(());
        };
        let commit = self.git_worktree.head_commit(workdir).await?;
        let check = self
            .assess_launch_trust(repo_root, &spec.start_point, commit.clone())
            .await?;
        if check.needs_confirmation() {
            if spec.confirmed_commit.as_deref() != Some(commit.as_str()) {
                tracing::warn!(
                    workdir = %workdir,
                    commit = %commit,
                    confirmed = ?spec.confirmed_commit,
                    repository_untrusted = check.repository_untrusted,
                    agent_config_changes = check.agent_config_changes.len(),
                    "not pre-accepting Claude Code's trust: the launch did not confirm this commit"
                );
                return Err(Error::LaunchNeedsConfirmation {
                    workdir: workdir.clone(),
                    commit,
                });
            }
            if check.repository_untrusted {
                self.git_worktree.record_dir_trust(repo_root).await?;
            }
        }
        self.git_worktree.ensure_dir_trusted(workdir).await
    }
}
