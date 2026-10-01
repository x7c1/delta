//! The fake's remembered-pane columns: the tmux pane a session row records
//! while the session is bound.

use delta_model::SessionId;

use crate::error::Result;
use crate::ports::RememberedPane;

use super::FakeStore;

impl FakeStore {
    pub(super) async fn remember_pane(&self, id: &SessionId, pane: &RememberedPane) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        // Mirror the real store's `UPDATE ... WHERE id`: a missing row records
        // nothing.
        if g.sessions.iter().any(|s| &s.id == id) {
            // …and, like it, takes the name away from any other row.
            g.remembered_panes
                .retain(|other, p| other == id || p.tmux_session != pane.tmux_session);
            g.remembered_panes.insert(id.clone(), pane.clone());
        }
        Ok(())
    }

    pub(super) async fn forget_pane(&self, id: &SessionId) -> Result<()> {
        self.inner.lock().unwrap().remembered_panes.remove(id);
        Ok(())
    }

    pub(super) async fn remembered_pane(&self, id: &SessionId) -> Result<Option<RememberedPane>> {
        Ok(self.inner.lock().unwrap().remembered_panes.get(id).cloned())
    }

    pub(super) async fn remembered_panes(&self) -> Result<Vec<(SessionId, RememberedPane)>> {
        let g = self.inner.lock().unwrap();
        // Oldest session first, like the real store: rows are kept in
        // insertion order, so walking them gives the creation order.
        Ok(g.sessions
            .iter()
            .filter_map(|s| {
                g.remembered_panes
                    .get(&s.id)
                    .map(|pane| (s.id.clone(), pane.clone()))
            })
            .collect())
    }
}
