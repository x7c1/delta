use delta_model::{MessageUuid, ThreadId};

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::provisional_branch_title;

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Resolve the delta-side thread a send lands on plus its semantic parent,
    /// creating the branch child thread when this is a branch send.
    ///
    /// This is delta's provider-neutral branch bookkeeping — a new thread lane
    /// (titled provisionally from the locator quote) carrying a `semantic_parent`
    /// back to the branched-from message. It is shared by **both** provider
    /// dispatch paths — Claude's pane enqueue ([`Self::enqueue_into_open`]) and
    /// the Codex adapter dispatch ([`super`]'s `enqueue_to_thread`) — so a branch
    /// produces the same delta-side structure regardless of provider. Only the
    /// turn dispatch and the hidden-context delivery differ per provider.
    ///
    /// A plain (non-branch) send returns `(thread_id, None)` unchanged and
    /// creates no thread.
    pub(in crate::interactor::enqueue) async fn resolve_branch_target(
        &self,
        thread_id: ThreadId,
        branch_from: Option<&MessageUuid>,
        locator_quote: Option<&str>,
    ) -> Result<(ThreadId, Option<MessageUuid>)> {
        match branch_from {
            Some(parent) => {
                // Give the new branch child a provisional title derived from the
                // locator quote so the navigator shows something meaningful until
                // it is renamed. Fall back to "untitled" when there is no quote.
                let title = provisional_branch_title(locator_quote);
                let thread = self
                    .store
                    .create_thread(self.id, &title, Some(thread_id))
                    .await?;
                Ok((thread.id, Some(parent.clone())))
            }
            None => Ok((thread_id, None)),
        }
    }
}
