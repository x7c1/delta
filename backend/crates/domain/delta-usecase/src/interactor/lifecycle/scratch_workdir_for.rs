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
    /// The scratch working directory for a spawn without a repository or a
    /// user-selected directory: `<base>/<key>`, keyed by the pane token for a
    /// Claude launch and by the session id for an adapter launch (which has no
    /// pane token).
    ///
    /// Distinct per spawn today, but no longer required to be: correlation is by
    /// the Delta-minted session id, not the workdir. Nothing exists there until
    /// the launch preparation creates it.
    pub(in crate::interactor::lifecycle) fn scratch_workdir_for(&self, key: &str) -> String {
        std::path::Path::new(&self.session_workdir_base)
            .join(key)
            .to_string_lossy()
            .into_owned()
    }
}
