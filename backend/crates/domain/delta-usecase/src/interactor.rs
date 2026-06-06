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

        // The caller-supplied thread must exist: for a plain send it is the
        // send target, for a branch send it is the parent the new child hangs
        // off. Validating here turns a stale/wrong id from the browser into a
        // clean `ThreadNotFound` (404) instead of an opaque foreign-key 500,
        // matching the read path's behaviour in `thread_view`.
        self.require_thread(thread_id).await?;

        let (target_thread, semantic_parent) = match branch_from {
            Some(parent) => {
                // Give the new branch child a provisional title derived from the
                // locator quote so the navigator shows something meaningful
                // until it is renamed. Fall back to "untitled" when there is no
                // quote.
                let title = provisional_branch_title(locator_quote);
                let thread = self
                    .store
                    .create_thread(&session.id, &title, Some(thread_id), Some(parent))
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

        // If the keystrokes never reach the pane, the row we just wrote would
        // sit at the head of the FIFO forever and block all future
        // `UserPromptSubmit` correlation. Roll it back to `cancelled` so the
        // head clears, then surface the original dispatch error.
        //
        // Best-effort: if the rollback itself fails we keep the dispatch error
        // (the caller's actionable failure) rather than masking it with a store
        // error. We do *not* roll back the just-created branch child thread: an
        // empty, unnamed thread is harmless overlay data and may legitimately be
        // reused by a retry, whereas the FIFO-blocking pending row is the actual
        // hazard this guard exists to clear.
        if let Err(dispatch_err) = self.tmux.send_line(text).await {
            let _ = self.store.cancel_send(pending.id).await;
            return Err(dispatch_err);
        }
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
    ///
    /// The locator quote to inject as `additionalContext` is resolved *before*
    /// syncing, by matching the prompt text against the queued `pending_send`
    /// (by text, not FIFO position). This is timing-independent: the quote is
    /// returned even when the user's transcript line has not been written yet.
    ///
    /// The actual message→thread attribution (and `mark_send_matched`) happens
    /// inside [`Self::sync_transcript`], keyed by matching each ingested user
    /// line to its queued send. A [`SessionEvent::TurnStarted`] is emitted when
    /// the user line for this prompt was attributed in this sync; otherwise the
    /// later `TurnCompleted` triggers the UI refetch. [`SessionEvent::ExternalInput`]
    /// is emitted only when no queued send matched this prompt at all.
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

        // Resolve this prompt's queued send *before* syncing, so the locator
        // quote is returned as `additionalContext` even when the user line has
        // not been ingested yet (the common timing case). Match by text — not by
        // FIFO head — so a stale send stuck at the head cannot suppress the quote
        // or misfire external-input detection.
        let pending = self
            .store
            .match_pending_send(&hook.session_id, hook.prompt.trim())
            .await?;
        let additional_context = pending.as_ref().and_then(|p| p.locator_quote.clone());

        // Ingest new transcript lines. This matches each user line to its queued
        // send and attributes it (plus the assistant lines that follow it) to
        // the right thread, marking the send matched as a side effect.
        let new_messages = self.sync_transcript(&hook.transcript_path).await?;

        match pending {
            Some(pending) => {
                // A queued send matches this prompt. If its user line was
                // attributed in this very sync, announce the turn now; otherwise
                // the line was not in the JSONL yet (the common timing case) and
                // the later `Stop` sync attributes it, with `TurnCompleted`
                // driving the UI refetch.
                if let Some(uuid) = match_uuid_for_prompt(&new_messages, &hook.prompt) {
                    events.push(SessionEvent::TurnStarted {
                        session_id: hook.session_id.clone(),
                        pending_send_id: pending.id,
                        matched_uuid: uuid,
                    });
                }
            }
            None => {
                // No queued send matched this prompt at all: external input.
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
        self.require_thread(thread_id).await?;
        self.store.thread_messages(thread_id).await
    }

    /// Pull new transcript lines from disk and persist them as messages,
    /// attributing each to the right thread as it is ingested.
    ///
    /// Attribution is driven by matching a user line's trimmed text to a queued
    /// `pending_send`, so it is robust regardless of which hook triggered the
    /// sync or whether the line was present when `UserPromptSubmit` fired.
    /// Lines are processed in order while maintaining `carry_thread`, the thread
    /// of the current turn:
    ///
    /// - A **user** line that matches a still-`pending` send is attributed to
    ///   that send's thread (the new child thread for a branch send), the send
    ///   is marked matched, and `carry_thread` advances to it. A user line with
    ///   no matching send is external input and lands on `main`, resetting
    ///   `carry_thread` to `main`.
    /// - A **non-user** line (assistant/tool/system) follows `carry_thread` —
    ///   the thread of the turn it belongs to.
    async fn sync_transcript(&self, transcript_path: &str) -> Result<Vec<Message>> {
        let session = self.require_session().await?;
        let main_thread = self.store.main_thread_id(&session.id).await?;
        let already = self.store.message_count(&session.id).await?;

        let lines = self.transcript.read_from(transcript_path, already).await?;
        if lines.is_empty() {
            return Ok(Vec::new());
        }

        // The turn in progress when this batch starts: the thread of the most
        // recent persisted user message, defaulting to `main`.
        let mut carry_thread = self
            .store
            .latest_user_thread(&session.id)
            .await?
            .unwrap_or(main_thread);

        let mut messages = Vec::with_capacity(lines.len());
        for (offset, line) in lines.into_iter().enumerate() {
            let seq = (already + offset) as i64;
            let content_text = Message::flatten_text(&line.content);

            let (thread_id, semantic_parent_uuid) =
                if matches!(line.role, delta_model::Role::User) {
                    let trimmed = content_text.as_deref().unwrap_or("").trim();
                    match self.store.match_pending_send(&session.id, trimmed).await? {
                        Some(pending) => {
                            self.store.mark_send_matched(pending.id, &line.uuid).await?;
                            carry_thread = pending.thread_id;
                            (pending.thread_id, pending.semantic_parent_uuid)
                        }
                        None => {
                            carry_thread = main_thread;
                            (main_thread, None)
                        }
                    }
                } else {
                    (carry_thread, None)
                };

            messages.push(Message {
                uuid: line.uuid,
                session_id: session.id.clone(),
                thread_id,
                role: line.role,
                linear_parent_uuid: line.linear_parent_uuid,
                semantic_parent_uuid,
                prompt_id: line.prompt_id,
                seq,
                content_text,
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

    /// Ensure a thread exists, turning a stale/wrong id into a clean
    /// `ThreadNotFound` instead of an opaque foreign-key error downstream.
    async fn require_thread(&self, thread_id: ThreadId) -> Result<()> {
        if self.store.thread(thread_id).await?.is_none() {
            return Err(Error::ThreadNotFound(thread_id.value()));
        }
        Ok(())
    }
}

/// Maximum length of a provisional branch title, in characters.
const PROVISIONAL_TITLE_MAX_CHARS: usize = 40;

/// Derive a provisional branch-thread title from a locator quote.
///
/// The quote is trimmed and truncated to [`PROVISIONAL_TITLE_MAX_CHARS`]
/// characters; an absent or blank quote falls back to `"untitled"`.
fn provisional_branch_title(locator_quote: Option<&str>) -> String {
    let trimmed = locator_quote.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        return "untitled".to_owned();
    }
    trimmed.chars().take(PROVISIONAL_TITLE_MAX_CHARS).collect()
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
