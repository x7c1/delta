use crate::error::Result;
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
    /// Mint a pane token whose tmux session name is not already in use.
    ///
    /// The minter's counter resets on each server start, but `delta-<n>` tmux
    /// sessions from a previous run can survive a restart — they are detached,
    /// so stopping the server does not kill them. Creating a tmux session with a
    /// name that already exists fails with "duplicate session", which would 500
    /// a spawn. So skip any minted name whose tmux session is still alive and
    /// advance to the next free one. The monotonic counter guarantees this
    /// terminates (there are finitely many surviving sessions) and that two
    /// concurrent spawns never contend for the same name.
    pub(in crate::interactor::lifecycle) async fn mint_free_token(&self) -> Result<PaneToken> {
        loop {
            let token = self.minter.mint();
            if !self.tmux.has_session(token.as_str()).await? {
                return Ok(token);
            }
        }
    }
}
