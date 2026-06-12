use delta_model::{Message, ThreadId};

use crate::error::Result;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::interactor::InteractorCore;

impl<T, X, S, W> InteractorCore<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Assemble a thread's transcript view (its messages ordered by `seq`).
    pub async fn thread_view(&self, thread_id: ThreadId) -> Result<Vec<Message>> {
        self.require_thread(thread_id).await?;
        self.store.thread_messages(thread_id).await
    }
}
