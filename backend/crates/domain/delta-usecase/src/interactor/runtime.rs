//! Simple accessors over the interactor's injected state, plus the
//! pane-input wipe that runs on a session's actor.
//!
//! EXCEPTION to the one-method-per-file rule: these are trivial accessors, so
//! they are grouped together rather than each given its own file.

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
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
    /// Borrow the store (useful for read-only queries from the transport layer).
    pub fn store(&self) -> &S {
        &self.store
    }

    #[cfg(test)]
    pub(crate) fn transcript(&self) -> &X {
        &self.transcript
    }

    #[cfg(test)]
    pub(crate) fn tmux(&self) -> &T {
        &self.tmux
    }

    #[cfg(test)]
    pub(crate) fn workspace(&self) -> &W {
        &self.workspace
    }

    #[cfg(test)]
    pub(crate) fn git_worktree(&self) -> &G {
        &self.git_worktree
    }
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Wipe the residual input of the session's pane, if it is open.
    ///
    /// When the session is open the pane's current input is cleared via the
    /// driver; when it is not open there is no live pane to clear, so this is
    /// a no-op returning `Ok(())`.
    ///
    /// Intended for use right before a fresh PTY attach: a prior client's detach
    /// leaves a focus-out (`ESC[O`) that Claude renders as a stray blank line, so
    /// clearing on the next attach keeps the input box clean across reconnects.
    pub(in crate::interactor) async fn clear_session_input(&mut self) -> Result<()> {
        if let Some(pane) = self.state.handle().map(|h| h.pane.clone()) {
            self.tmux.clear_input(&pane).await?;
        }
        Ok(())
    }
}
