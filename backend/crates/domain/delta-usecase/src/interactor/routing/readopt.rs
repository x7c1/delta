//! Boot-time re-adoption of the sessions whose panes survived a restart.

use std::collections::HashSet;

use crate::error::Result;
use crate::interactor::lifecycle::Readoption;
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::ReadoptionSummary;

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Re-adopt every session whose remembered pane is still running on
    /// Delta's tmux server, before the server accepts requests.
    ///
    /// A Claude Code session's pane outlives the Delta process (see the
    /// `adopt_pane` lifecycle module). For each session row that remembers a
    /// pane, the session's actor is asked to look for it: a live pane is bound
    /// without launching anything and its transcript caught up from the stored
    /// cursor, and the actor stays up, so the transcript tail, the liveness
    /// probe and the echo watchdog take over from there; a gone pane has its
    /// record cleared and the session stays closed.
    ///
    /// `hook_endpoint_changed` is the server's report that its hook URLs differ
    /// from the previous run's (`delta_bootstrap::Config::hook_endpoint_changed`).
    /// It marks only the sessions actually re-adopted — the flag is also true on
    /// a first run, when there is nothing to strand.
    ///
    /// Running this before serving is what keeps a send from racing it; the
    /// resume backstop in `open_session` covers any send that does anyway.
    ///
    /// One session's failure does not stop the others: it is logged and
    /// counted. The rows are walked newest first, and a tmux session name a
    /// newer row already claimed is forgotten on the older one, since only the
    /// newer row can own a name that was minted again after the older pane
    /// died. Only failing to list the remembered panes at all is an `Err`.
    ///
    /// Codex sessions have nothing to re-adopt: their `codex app-server` is a
    /// child of the Delta process and ends with it, and their rows never
    /// remember a pane. The next send resumes their thread as usual.
    pub async fn readopt_surviving_sessions(
        &self,
        hook_endpoint_changed: bool,
    ) -> Result<ReadoptionSummary> {
        let mut summary = ReadoptionSummary::default();
        let mut claimed = HashSet::new();
        for (id, remembered) in self.store.remembered_panes().await?.into_iter().rev() {
            if !claimed.insert(remembered.tmux_session.clone()) {
                tracing::warn!(
                    session_id = %id,
                    token = %remembered.tmux_session,
                    "two sessions remember the same tmux session; the newer one keeps \
                     it and this one is left closed"
                );
                if let Err(err) = self.store.forget_pane(&id).await {
                    tracing::warn!(
                        session_id = %id,
                        error = %err,
                        "could not clear a duplicated pane record"
                    );
                }
                summary.failed += 1;
                continue;
            }
            let outcome = self
                .request(&id, |reply| SessionInput::ReadoptPane {
                    hook_endpoint_changed,
                    reply,
                })
                .await;
            match outcome {
                Ok(Readoption::Adopted { hooks_unreachable }) => {
                    summary.adopted += 1;
                    if hooks_unreachable {
                        summary.hooks_unreachable += 1;
                    }
                }
                Ok(Readoption::Gone) => summary.gone += 1,
                Ok(Readoption::Unprobed) => summary.unprobed += 1,
                Ok(Readoption::NotRemembered) => {}
                Err(err) => {
                    tracing::warn!(
                        session_id = %id,
                        error = %err,
                        "re-adopting a session that may have survived the restart failed; \
                         it stays closed"
                    );
                    summary.failed += 1;
                }
            }
        }
        Ok(summary)
    }
}
