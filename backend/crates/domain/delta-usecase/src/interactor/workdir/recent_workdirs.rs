use crate::error::Result;
use crate::ports::{RecentWorkdir, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

use super::RECENT_WORKDIRS_LIMIT;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// The recently-used working directories for the picker's "recent" list.
    ///
    /// Distinct `session.cwd` values, most-recently-used first, capped at
    /// [`RECENT_WORKDIRS_LIMIT`]. Derived from existing session rows (Delta keeps
    /// no separate history), so a directory appears here once any session has run
    /// in it.
    pub async fn recent_workdirs(&self) -> Result<Vec<RecentWorkdir>> {
        self.store.recent_workdirs(RECENT_WORKDIRS_LIMIT).await
    }
}
