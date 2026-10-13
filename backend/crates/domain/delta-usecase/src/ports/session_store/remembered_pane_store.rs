//! The pane a session row remembers across restarts.

use async_trait::async_trait;

use delta_model::SessionId;

use super::SessionStore;
use crate::error::Result;
use crate::ports::remembered_pane::RememberedPane;

/// The tmux pane each session row remembers, so a restarted Delta can
/// re-adopt it.
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait RememberedPaneStore: Send + Sync {
    /// Record the tmux pane a session is bound to, replacing whatever was
    /// recorded before. A missing row is a no-op.
    ///
    /// Written whenever a pane-backed session is bound — a fresh spawn's first
    /// hook, a resume, a re-adoption after a restart — so that the row always
    /// names the pane the session's agent is running in. See
    /// [`RememberedPane`] for why this is persisted at all.
    ///
    /// Also clears the record of any *other* row that names the same tmux
    /// session, in the same write. Delta reuses `delta-<n>` names across
    /// restarts (the minter skips only names tmux still knows), so another row
    /// naming this one can only be remembering a pane that died before the
    /// name was minted again — a record left stale by a failed clear or a
    /// probe that could not run. Kept, it would make that row's next send
    /// adopt this session's pane as its own.
    async fn remember_pane(&self, id: &SessionId, pane: &RememberedPane) -> Result<()>;

    /// Erase the session's remembered pane: the session was torn down (its
    /// pane killed, or found gone), so no pane is running for it any more. A
    /// row with nothing remembered, or a missing row, is a no-op.
    async fn forget_pane(&self, id: &SessionId) -> Result<()>;

    /// The pane the session's row remembers, or `None` when nothing is
    /// recorded (closed, never bound, adapter-backed) or the row is missing.
    async fn remembered_pane(&self, id: &SessionId) -> Result<Option<RememberedPane>>;

    /// Every session whose row remembers a pane, oldest session first: the
    /// candidates a restarted Delta tries to re-adopt.
    async fn remembered_panes(&self) -> Result<Vec<(SessionId, RememberedPane)>>;
}

#[async_trait]
impl RememberedPaneStore for Box<dyn SessionStore> {
    async fn remember_pane(&self, id: &SessionId, pane: &RememberedPane) -> Result<()> {
        (**self).remember_pane(id, pane).await
    }

    async fn forget_pane(&self, id: &SessionId) -> Result<()> {
        (**self).forget_pane(id).await
    }

    async fn remembered_pane(&self, id: &SessionId) -> Result<Option<RememberedPane>> {
        (**self).remembered_pane(id).await
    }

    async fn remembered_panes(&self) -> Result<Vec<(SessionId, RememberedPane)>> {
        (**self).remembered_panes().await
    }
}
