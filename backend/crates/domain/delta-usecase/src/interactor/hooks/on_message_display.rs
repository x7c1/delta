use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{MessageDisplayHook, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W> SessionContext<'_, T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Handle a `MessageDisplay` hook: one chunk of the in-flight turn's
    /// assistant message, delivered live before the transcript is flushed.
    ///
    /// The hook is passive — it never mutates the TUI display — so the handler
    /// only buffers the chunk as a provisional live preview and re-broadcasts
    /// it as an [`SessionEvent::AssistantStreaming`] for the browser. The
    /// transport answers an empty 200; nothing here is persisted.
    ///
    /// The preview is attributed to the in-flight turn's thread, recovered the
    /// same way the transcript sync recovers its carry thread: the thread of
    /// the latest persisted user message, falling back to the session's main
    /// thread. The hook's `message_id` does not match any transcript id, so the
    /// preview is reconciled per turn (cleared when the turn ends) rather than
    /// id-joined to the eventually-persisted message.
    ///
    /// An unknown session (no row yet, e.g. a chunk racing ahead of the first
    /// `UserPromptSubmit` bind) is a safe no-op: there is no thread to attribute
    /// to, so nothing is buffered or broadcast.
    pub(in crate::interactor) async fn on_message_display(
        &mut self,
        hook: MessageDisplayHook,
    ) -> Result<Vec<SessionEvent>> {
        let Some(session) = self.store.session(&hook.session_id).await? else {
            return Ok(Vec::new());
        };
        let main_thread = self.store.main_thread_id(&session.id).await?;
        let thread_id = self
            .store
            .latest_user_thread(&session.id)
            .await?
            .unwrap_or(main_thread);

        self.state.accumulate_streaming(
            &hook.message_id,
            thread_id,
            hook.index,
            hook.final_,
            hook.delta.clone(),
        );

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
