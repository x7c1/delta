//! The [`Interactor`]: orchestrates the ports into Delta's use cases.

use delta_model::{Message, MessageUuid, PendingSend, Thread, ThreadId};

use crate::error::{Error, Result};
use crate::ports::{
    NewSession, SessionEvent, SessionStore, StopHook, TmuxDriver, Transcript, UserPromptSubmitHook,
};

/// Holds the injected capabilities and exposes Delta's use cases.
///
/// Generic over the three ports so callers can inject any implementation. The
/// composition root and the application share a single concrete type through
/// the [`BoxedInteractor`] alias, which erases the gateways behind trait
/// objects; this keeps the transport layer's shared state non-generic while
/// still allowing tests to substitute fakes.
pub struct Interactor<T, X, S> {
    tmux: T,
    transcript: X,
    store: S,
}

/// An [`Interactor`] with its three ports type-erased behind trait objects.
///
/// Both the production composition root and integration tests build this exact
/// type, so the transport layer's shared state stays non-generic regardless of
/// which gateways are wired in.
pub type BoxedInteractor = Interactor<
    Box<dyn TmuxDriver>,
    Box<dyn Transcript>,
    Box<dyn SessionStore>,
>;

impl<T, X, S> Interactor<T, X, S>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
{
    /// Construct an Interactor from the three injected ports.
    pub fn new(tmux: T, transcript: X, store: S) -> Self {
        Self {
            tmux,
            transcript,
            store,
        }
    }

    /// Borrow the store (useful for read-only queries from the transport layer).
    pub fn store(&self) -> &S {
        &self.store
    }

    #[cfg(test)]
    pub(crate) fn transcript(&self) -> &X {
        &self.transcript
    }

    /// Enqueue a user input to be sent into the session.
    ///
    /// If `branch_from` is set, this is the first message of a new branch: an
    /// unnamed child thread is created off that message and the send is
    /// attributed to it. Otherwise the send targets the given `thread_id`
    /// (typically `main`). The send is written to the FIFO *before* the
    /// keystrokes are dispatched, so the correlation head is in place when the
    /// `UserPromptSubmit` hook fires.
    pub async fn enqueue_send(
        &self,
        thread_id: ThreadId,
        text: &str,
        locator_quote: Option<&str>,
        branch_from: Option<&MessageUuid>,
    ) -> Result<PendingSend> {
        let session = self.require_session().await?;

        let (target_thread, semantic_parent) = match branch_from {
            Some(parent) => {
                let thread = self
                    .store
                    .create_thread(&session.id, "untitled", Some(thread_id), Some(parent))
                    .await?;
                (thread.id, Some(parent.clone()))
            }
            None => (thread_id, None),
        };

        let pending = self
            .store
            .enqueue_send(
                &session.id,
                target_thread,
                semantic_parent.as_ref(),
                text,
                locator_quote,
            )
            .await?;

        self.tmux.send_line(text).await?;
        Ok(pending)
    }

    /// Create a named branch off an existing message without sending anything.
    pub async fn create_branch(
        &self,
        parent_thread_id: ThreadId,
        root_message_uuid: &MessageUuid,
        title: &str,
    ) -> Result<Thread> {
        let session = self.require_session().await?;
        self.store
            .create_thread(
                &session.id,
                title,
                Some(parent_thread_id),
                Some(root_message_uuid),
            )
            .await
    }

    /// Handle a `UserPromptSubmit` hook.
    ///
    /// The first such hook registers the session (SessionStart never fires).
    /// The prompt is matched against the FIFO head of `pending_send`: on a hit
    /// the send is marked matched and a [`SessionEvent::TurnStarted`] is
    /// returned, optionally carrying the locator quote to inject as
    /// `additionalContext`; on a miss it is treated as external input.
    ///
    /// Returns the events to broadcast and, when a locator quote should be
    /// injected, the `additionalContext` string for the hook response.
    pub async fn on_user_prompt_submit(
        &self,
        hook: UserPromptSubmitHook,
    ) -> Result<(Vec<SessionEvent>, Option<String>)> {
        let mut events = Vec::new();

        let existing = self.store.current_session().await?;
        if existing.is_none() {
            self.store
                .register_session(NewSession {
                    id: hook.session_id.clone(),
                    cwd: hook.cwd.clone(),
                    transcript_path: hook.transcript_path.clone(),
                })
                .await?;
            events.push(SessionEvent::SessionRegistered {
                session_id: hook.session_id.clone(),
            });
        }

        // Ingest any new transcript lines so the matched uuid is available.
        let new_messages = self.sync_transcript(&hook.transcript_path).await?;

        // Correlate against the FIFO head.
        let head = self.store.head_pending_send(&hook.session_id).await?;
        let mut additional_context = None;

        match head {
            Some(pending) if prompt_matches(&pending.text, &hook.prompt) => {
                let matched_uuid = match_uuid_for_prompt(&new_messages, &hook.prompt);
                if let Some(uuid) = matched_uuid {
                    self.store.mark_send_matched(pending.id, &uuid).await?;
                    events.push(SessionEvent::TurnStarted {
                        session_id: hook.session_id.clone(),
                        pending_send_id: pending.id,
                        matched_uuid: uuid,
                    });
                }
                additional_context = pending.locator_quote.clone();
            }
            _ => {
                events.push(SessionEvent::ExternalInput {
                    session_id: hook.session_id.clone(),
                    prompt: hook.prompt.clone(),
                });
            }
        }

        Ok((events, additional_context))
    }

    /// Handle a `Stop` hook: ingest the final transcript lines and report the
    /// turn as completed.
    pub async fn on_stop(&self, hook: StopHook) -> Result<Vec<SessionEvent>> {
        if let Some(session) = self.store.current_session().await? {
            self.sync_transcript(&session.transcript_path).await?;
        }
        Ok(vec![SessionEvent::TurnCompleted {
            session_id: hook.session_id,
            stop_reason: hook.stop_reason,
        }])
    }

    /// Handle a `PreToolUse` hook: record the request for UI/audit and notify
    /// the browser. Delta never returns allow/deny — the TUI owns that.
    pub async fn on_pre_tool_use(
        &self,
        session_id: &delta_model::SessionId,
        tool_name: &str,
        tool_input_json: &str,
    ) -> Result<Vec<SessionEvent>> {
        let request = self
            .store
            .record_permission_request(session_id, tool_name, tool_input_json)
            .await?;
        Ok(vec![SessionEvent::PermissionRequested {
            session_id: session_id.clone(),
            request_id: request.id,
            tool_name: tool_name.to_owned(),
        }])
    }

    /// The current session and the id of its `main` thread, for hydration.
    ///
    /// Returns `None` until the first `UserPromptSubmit` hook registers the
    /// session (Claude Code never fires `SessionStart`).
    pub async fn current_session(&self) -> Result<Option<(delta_model::Session, ThreadId)>> {
        match self.store.current_session().await? {
            Some(session) => {
                let main = self.store.main_thread_id(&session.id).await?;
                Ok(Some((session, main)))
            }
            None => Ok(None),
        }
    }

    /// The thread tree for the current session, ordered by creation.
    ///
    /// Returns an empty list when no session has been registered yet.
    pub async fn threads(&self) -> Result<Vec<Thread>> {
        match self.store.current_session().await? {
            Some(session) => self.store.list_threads(&session.id).await,
            None => Ok(Vec::new()),
        }
    }

    /// Assemble a thread's transcript view (its messages ordered by `seq`).
    pub async fn thread_view(&self, thread_id: ThreadId) -> Result<Vec<Message>> {
        if self.store.thread(thread_id).await?.is_none() {
            return Err(Error::ThreadNotFound(thread_id.value()));
        }
        self.store.thread_messages(thread_id).await
    }

    /// Pull new transcript lines from disk and persist them as messages,
    /// attaching the active thread (currently the session's `main` thread) and
    /// the next sequence numbers. Returns the newly ingested messages.
    async fn sync_transcript(&self, transcript_path: &str) -> Result<Vec<Message>> {
        let session = self.require_session().await?;
        let main_thread = self.store.main_thread_id(&session.id).await?;
        let already = self.store.message_count(&session.id).await?;

        let lines = self.transcript.read_from(transcript_path, already).await?;
        if lines.is_empty() {
            return Ok(Vec::new());
        }

        let mut messages = Vec::with_capacity(lines.len());
        for (offset, line) in lines.into_iter().enumerate() {
            let seq = (already + offset) as i64;
            messages.push(Message {
                uuid: line.uuid,
                session_id: session.id.clone(),
                thread_id: main_thread,
                role: line.role,
                linear_parent_uuid: line.linear_parent_uuid,
                semantic_parent_uuid: None,
                prompt_id: line.prompt_id,
                seq,
                content_text: Message::flatten_text(&line.content),
                content: line.content,
                created_at: line.created_at.unwrap_or_default(),
            });
        }

        self.store.upsert_messages(&messages).await?;
        Ok(messages)
    }

    async fn require_session(&self) -> Result<delta_model::Session> {
        self.store.current_session().await?.ok_or(Error::NoSession)
    }
}

/// Whether a hook prompt corresponds to a queued send.
///
/// Claude Code may trim trailing whitespace, so compare on the trimmed text.
fn prompt_matches(pending_text: &str, hook_prompt: &str) -> bool {
    pending_text.trim() == hook_prompt.trim()
}

/// Find the transcript uuid for the user line carrying this prompt.
fn match_uuid_for_prompt(messages: &[Message], prompt: &str) -> Option<MessageUuid> {
    messages
        .iter()
        .rev()
        .find(|m| {
            matches!(m.role, delta_model::Role::User)
                && m.content_text.as_deref().map(str::trim) == Some(prompt.trim())
        })
        .map(|m| m.uuid.clone())
}

#[cfg(test)]
mod tests;
