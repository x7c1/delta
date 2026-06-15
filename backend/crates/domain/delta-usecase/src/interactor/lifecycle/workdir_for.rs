use crate::interactor::InteractorCore;
use crate::pane_token::PaneToken;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// The working directory for a spawn: `<base>/<token>`.
    ///
    /// Distinct per spawn today, but no longer required to be: correlation is by
    /// the Delta-minted session id, not the workdir.
    pub(in crate::interactor::lifecycle) fn workdir_for(&self, token: &PaneToken) -> String {
        std::path::Path::new(&self.session_workdir_base)
            .join(token.as_str())
            .to_string_lossy()
            .into_owned()
    }
}
