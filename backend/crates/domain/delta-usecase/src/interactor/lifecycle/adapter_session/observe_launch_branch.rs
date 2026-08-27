//! Reading the git branch of an adapter-backed session's launch directory,
//! once that directory really exists.

use delta_model::SessionId;

use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// The git branch checked out in an adapter-backed session's launch
    /// directory, for stamping on the messages it produces.
    ///
    /// Observed at bind time, once the directory really exists (a worktree
    /// spawn's does not until the launch task has built it), rather than read
    /// from the session row's `branch_at_launch`: that column is only filled on
    /// the worktree spawn path, so a session started in a plain git directory
    /// would report no branch at all despite obviously having one. Observing on
    /// every bind also keeps a resumed session honest, where the spawn-time
    /// snapshot could be stale.
    ///
    /// Delta observes this itself because no adapter-backed provider reports it.
    /// Codex's `thread/start` response *declares* a `thread.gitInfo` — the schema
    /// even documents it as "captured when the thread was created" — but the real
    /// server returns it as `null` there (verified against `codex-cli 0.144.4`),
    /// so waiting for the provider to report a branch means never reporting one.
    /// Asking git about a directory Delta itself chose is not reconstructing a
    /// provider fact; it is Delta reporting what it observed about its own launch
    /// site.
    ///
    /// A git failure is **not** fatal: `None` is already the honest answer for a
    /// directory that is not a git working tree or has a detached HEAD, so a
    /// broken or missing `git` degrades to the same absent metadata rather than
    /// failing a session the user asked for. It is logged, because "no branch
    /// shown" caused by a broken git should be diagnosable rather than silent.
    pub(in crate::interactor) async fn observe_launch_branch(
        &self,
        session_id: &SessionId,
        cwd: &str,
    ) -> Option<String> {
        match self.git_worktree.current_branch(cwd).await {
            Ok(branch) => branch,
            Err(err) => {
                tracing::warn!(
                    session_id = %session_id,
                    cwd = %cwd,
                    error = %err,
                    "could not observe the launch directory's git branch; \
                     the session's messages will report no branch"
                );
                None
            }
        }
    }
}
