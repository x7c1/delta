use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{
    GitWorktree, MessageDisplayHook, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace,
};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Handle a `MessageDisplay` hook: one chunk of the in-flight turn's
    /// assistant message, delivered live before the transcript is flushed.
    ///
    /// The hook is passive — it never mutates the TUI display — so the handler
    /// only re-broadcasts the chunk as an [`SessionEvent::AssistantStreaming`]
    /// for the browser, which accumulates the provisional live preview by
    /// chunk index. The server keeps no copy of the text; the transport
    /// answers an empty 200; nothing here is persisted.
    ///
    /// The preview is attributed to the in-progress turn's thread (see
    /// [`SessionStore::in_progress_turn_thread`]): the in-flight send's thread,
    /// else the latest persisted user message, else the session's main thread.
    /// The in-flight-send step keeps a mid-turn branch turn's stream on the new
    /// branch thread, whose user line is not yet ingested. The hook's
    /// `message_id` does not match any transcript id, so the browser reconciles
    /// the preview per turn rather than id-joining it to the eventually-persisted
    /// message.
    ///
    /// An unknown session (no row yet, e.g. a chunk racing ahead of the first
    /// `UserPromptSubmit` bind) is a safe no-op: there is no thread to attribute
    /// to, so nothing is broadcast.
    pub(in crate::interactor) async fn on_message_display(
        &mut self,
        hook: MessageDisplayHook,
    ) -> Result<Vec<SessionEvent>> {
        let Some(session) = self.store.session(&hook.session_id).await? else {
            return Ok(Vec::new());
        };
        let thread_id = self.store.in_progress_turn_thread(&session.id).await?;

        Ok(vec![SessionEvent::AssistantStreaming {
            session_id: hook.session_id,
            thread_id,
            message_id: hook.message_id,
            index: hook.index,
            final_: hook.final_,
            delta: hook.delta,
        }])
    }
}
