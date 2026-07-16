//! The interactor's public surface, routed through the per-session actors.
//!
//! Every state-mutating (or runtime-state-reading) use case becomes "post one
//! [`SessionInput`] to the owning actor, await its reply", so the public
//! method signatures are unchanged while per-session ordering is enforced by
//! the actor mailbox instead of lock discipline. Pure store reads (listing,
//! threads, messages, workdir browsing) do **not** come through here — they
//! stay direct on the core (see the `listing`/`workdir` modules), reachable
//! via the interactor's `Deref`.

use std::time::Instant;

use delta_model::{Message, Send, SessionId};
use tokio::sync::oneshot;

use crate::error::{Error, Result};
use crate::interactor::hooks::PermissionWait;
use crate::interactor::session_actor::input::{Reply, SessionInput};
use crate::interactor::session_actor::runtime::SessionLiveState;
use crate::interactor::{Interactor, PermissionDecision};
use crate::pane_token::PaneToken;
use crate::ports::{
    GitWorktree, MessageDisplayHook, SessionEndHook, SessionEvent, SessionLifecycle,
    SessionStartHook, SessionStore, StopHook, TmuxDriver, Transcript, UserPromptSubmitHook,
    Workspace,
};
use crate::send_target::SendTarget;
use crate::turn::TurnState;

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Post an input to the session's actor (spawning it on first contact)
    /// and await the result.
    async fn request<R>(
        &self,
        id: &SessionId,
        make: impl FnOnce(Reply<R>) -> SessionInput,
    ) -> Result<R> {
        let (tx, rx) = oneshot::channel();
        self.sessions.post(id, make(tx));
        match rx.await {
            Ok(result) => result,
            // Only reachable during tear-down (the input was dropped) or if
            // the actor panicked mid-handling.
            Err(_) => Err(Error::Internal(format!(
                "session {id} actor dropped before replying"
            ))),
        }
    }

    /// Read a piece of runtime state from the session's actor, substituting
    /// `default` when the session has no actor (closed/idle by definition).
    async fn query<R>(
        &self,
        id: &SessionId,
        make: impl FnOnce(oneshot::Sender<R>) -> SessionInput,
        default: R,
    ) -> R {
        let (tx, rx) = oneshot::channel();
        if !self.sessions.post_existing(id, make(tx)) {
            return default;
        }
        rx.await.unwrap_or(default)
    }

    /// Mint a fresh Claude `session_id` for a spawn: a time-ordered UUID v7
    /// (a 48-bit millisecond timestamp prefix followed by random bits), so
    /// session ids sort chronologically by creation time while remaining
    /// fully valid RFC 9562 UUIDs, and collision with an existing stored
    /// session is astronomically unlikely. Minted here — before the session's
    /// actor exists — because the id *is* the actor's routing key.
    fn mint_session_id() -> SessionId {
        SessionId::from(uuid::Uuid::now_v7().to_string())
    }

    // ---- API commands ------------------------------------------------------

    /// Enqueue a user input, routing it to the session the target names.
    ///
    /// The session is determined by the [`SendTarget`], never by a global
    /// "current" session:
    ///
    /// - [`SendTarget::Thread`] — an existing conversation. The owning session
    ///   is derived from the thread here (a read), then the enqueue executes
    ///   on that session's actor: ensured open (resumed if closed), the `send`
    ///   row written before the keystrokes, deferred `queued` when a turn is
    ///   in flight.
    /// - [`SendTarget::NewSession`] — a composer-first message. A fresh
    ///   session id is minted, its actor spawned, and the launch executes
    ///   there: the session row (status `spawning`), its `main` thread, and
    ///   the `send` row are all written *before* `claude` launches, so the
    ///   returned [`Send`] carries real ids.
    ///
    /// Returns the created send plus any [`SessionEvent`]s the enqueue
    /// produced; the transport broadcasts them.
    pub async fn enqueue_send(
        &self,
        target: SendTarget,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<(Send, Vec<SessionEvent>)> {
        match target {
            SendTarget::Thread {
                thread_id,
                branch_from,
            } => {
                // Derive the owning session from the target thread. A stale or
                // wrong id becomes a clean `ThreadNotFound` (404) rather than
                // an opaque failure downstream.
                let thread = self
                    .store
                    .thread(thread_id)
                    .await?
                    .ok_or_else(|| Error::ThreadNotFound(thread_id.value()))?;
                let session_id = thread.session_id;
                let text = text.to_owned();
                let locator_quote = locator_quote.map(str::to_owned);
                self.request(&session_id, move |reply| SessionInput::EnqueueToThread {
                    thread_id,
                    branch_from,
                    text,
                    locator_quote,
                    reply,
                })
                .await
            }
            SendTarget::NewSession {
                workdir,
                launch_option_ids,
                worktree,
                provider,
            } => {
                // `locator_quote` is intentionally dropped here, not forwarded
                // to the spawn: a brand-new session has no earlier passage to
                // anchor, so there is nothing to locate. The persisted row
                // (and therefore the response) carries no quote.
                let id = Self::mint_session_id();
                let text = text.to_owned();
                let spawn = self
                    .request(&id, move |reply| SessionInput::SpawnFresh {
                        first_prompt: Some(text),
                        workdir,
                        launch_option_ids,
                        worktree,
                        provider,
                        reply,
                    })
                    .await?;
                let send = spawn
                    .first_send
                    .expect("spawn_fresh enqueues a send when a first prompt is given");
                Ok((send, Vec::new()))
            }
        }
    }

    /// Spawn a fresh Claude Code session with no initial send (cold-start).
    pub async fn new_session(&self) -> Result<PaneToken> {
        let id = Self::mint_session_id();
        let spawn = self
            .request(&id, |reply| SessionInput::SpawnFresh {
                first_prompt: None,
                workdir: None,
                // A cold-start session (no first prompt) applies no launch
                // options; those ride only on a composer-initiated new session.
                launch_option_ids: Vec::new(),
                // A cold-start session never opts into a worktree (no workdir,
                // no first prompt); the worktree path rides only on a
                // composer-initiated new session.
                worktree: None,
                // Cold start is the Claude tmux path; a Codex session is only
                // ever created from a composer-initiated new session (which
                // carries a first prompt and its own provider selection).
                provider: delta_model::AgentProvider::Claude,
                reply,
            })
            .await?;
        Ok(spawn
            .token
            .expect("a Claude cold-start spawn always mints a pane token"))
    }

    /// Ensure at least one Claude Code session is up, spawning one if absent.
    ///
    /// Idempotent for the still-single server surface: if any session's actor
    /// holds a live pane (bound or pending spawn) it is reused and
    /// [`SessionLifecycle::Ready`] is returned with no side effects. Otherwise
    /// a fresh session is spawned and [`SessionLifecycle::Starting`] returned.
    pub async fn ensure_session(&self) -> Result<SessionLifecycle> {
        for id in self.sessions.ids() {
            if self
                .query(&id, |reply| SessionInput::QueryIsLive { reply }, false)
                .await
            {
                return Ok(SessionLifecycle::Ready);
            }
        }
        self.new_session().await?;
        Ok(SessionLifecycle::Starting)
    }

    /// Resume a closed but known session under a fresh tmux session.
    pub async fn open_session(&self, id: &SessionId) -> Result<PaneToken> {
        self.request(id, |reply| SessionInput::OpenSession { reply })
            .await
    }

    /// Close an open session: capture its final transcript line, kill its
    /// pane, and drop its binding. The conversational data remains in the
    /// store. Unknown ids are a clean `SessionNotFound`.
    ///
    /// Returns any [`SessionEvent::SubagentFinished`]s the process-gone sweep
    /// produced (a lingering background subagent cleared because its completion
    /// notification can no longer arrive); the transport broadcasts them.
    pub async fn close_session(&self, id: &SessionId) -> Result<Vec<SessionEvent>> {
        self.request(id, |reply| SessionInput::CloseSession { reply })
            .await
    }

    /// Wipe the residual input of a session's pane, if it is open. A no-op
    /// (returning `Ok`) when the session is not open — including when it has
    /// no actor at all.
    pub async fn clear_session_input(&self, id: &SessionId) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        if !self
            .sessions
            .post_existing(id, SessionInput::ClearInput { reply: tx })
        {
            return Ok(());
        }
        rx.await.unwrap_or(Ok(()))
    }

    // ---- Runtime-state queries ----------------------------------------------

    /// The pane driving a specific open session, if it is open.
    ///
    /// The PTY bridge routes by session id: with no live pane for `id` this
    /// returns `None` so the bridge can refuse the attach rather than bind to
    /// a non-existent pane.
    pub async fn pane_for_session(&self, id: &SessionId) -> Option<String> {
        self.query(id, |reply| SessionInput::QueryPane { reply }, None)
            .await
    }

    /// Whether a session is currently open (driven by a live pane).
    ///
    /// Open/closed is process-runtime state owned by the session's actor, so
    /// this is the authority the session-list endpoint annotates each stored
    /// session with; a session with no actor is closed by definition.
    pub async fn is_session_open(&self, id: &SessionId) -> bool {
        self.query(id, |reply| SessionInput::QueryIsOpen { reply }, false)
            .await
    }

    /// The queryable live state of a session: its turn phase plus the
    /// pending permission dialog, snapshotted in one actor message.
    ///
    /// Public because the REST surface reports it (the sends envelope
    /// carries `turn` and `permission`, so the browser can rebuild its
    /// in-progress indicator and permission notice after a reconnect). A
    /// session with no actor is idle with nothing pending by definition.
    pub async fn live_state_for(&self, id: &SessionId) -> SessionLiveState {
        // Resolved inline (rather than through `query`) so each outcome can be
        // logged: a captured debug log must distinguish a state the live actor
        // actually returned from the default `Idle` substituted when no actor
        // is reachable. Without this, an `Idle` in a report is ambiguous —
        // genuinely idle, or a silent fallback?
        let default = SessionLiveState {
            turn: TurnState::Idle,
            in_progress_thread: None,
            pending_permission: None,
            pending_question: None,
            running_subagents: Vec::new(),
        };
        let (tx, rx) = oneshot::channel();
        if !self
            .sessions
            .post_existing(id, SessionInput::QueryLiveState { reply: tx })
        {
            tracing::debug!(
                session_id = %id,
                branch = "no_actor",
                "live_state_for: no session actor; returning default Idle (a session \
                 with no actor is idle with nothing pending by definition)"
            );
            return default;
        }
        match rx.await {
            Ok(state) => {
                tracing::debug!(
                    session_id = %id,
                    branch = "actor_reply",
                    turn = ?state.turn,
                    has_pending_permission = state.pending_permission.is_some(),
                    has_pending_question = state.pending_question.is_some(),
                    "live_state_for: state from live actor"
                );
                state
            }
            Err(_) => {
                tracing::debug!(
                    session_id = %id,
                    branch = "dropped_reply",
                    "live_state_for: actor existed but dropped its reply (retiring \
                     mid-query); returning default Idle"
                );
                default
            }
        }
    }

    // ---- Hook deliveries -----------------------------------------------------

    /// Handle a `UserPromptSubmit` hook, routed to the session the hook names
    /// (spawning its actor on first contact, which is what registers an
    /// external session). Returns the events to broadcast and, when a locator
    /// quote should be injected, the `additionalContext` string.
    pub async fn on_user_prompt_submit(
        &self,
        hook: UserPromptSubmitHook,
    ) -> Result<(Vec<SessionEvent>, Option<String>)> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::UserPromptSubmit {
            hook,
            reply,
        })
        .await
    }

    /// Handle a `Stop` hook: ingest the final transcript lines and report the
    /// turn as completed.
    pub async fn on_stop(&self, hook: StopHook) -> Result<Vec<SessionEvent>> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::Stop { hook, reply })
            .await
    }

    /// Handle a `MessageDisplay` hook: buffer one chunk of the in-flight turn's
    /// assistant message and return the `AssistantStreaming` event to broadcast.
    pub async fn on_message_display(&self, hook: MessageDisplayHook) -> Result<Vec<SessionEvent>> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::MessageDisplay {
            hook,
            reply,
        })
        .await
    }

    /// Handle a `SessionStart` hook (launch/resume readiness signal).
    pub async fn on_session_start(&self, hook: SessionStartHook) -> Result<Vec<SessionEvent>> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::SessionStart { hook, reply })
            .await
    }

    /// Handle a `SessionEnd` hook (early launch-failure signal / normal end).
    pub async fn on_session_end(&self, hook: SessionEndHook) -> Result<Vec<SessionEvent>> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::SessionEnd { hook, reply })
            .await
    }

    /// Handle a `PreToolUse` hook: record the permission request (with its
    /// `tool_use_id`) so the later `tool_result` can resolve it. Routed
    /// through the session's mailbox so the write is ordered with ingestion.
    pub async fn on_pre_tool_use(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: &str,
        transcript_path: &str,
    ) -> Result<Vec<SessionEvent>> {
        self.request(session_id, |reply| SessionInput::PreToolUse {
            tool_name: tool_name.to_owned(),
            tool_input_json: tool_input_json.to_owned(),
            tool_use_id: tool_use_id.to_owned(),
            transcript_path: transcript_path.to_owned(),
            reply,
        })
        .await
    }

    /// Handle a `PostToolUse` hook: a tool call completed. Delta acts on the
    /// subagent (`Agent`/`Task`) case in two ways. For a foreground subagent
    /// the running window is closed by `tool_use_id`. For a background subagent
    /// the call returned, not the subagent — so the matching launch row is
    /// upgraded with the `agentId` the tool's `tool_result` carries, giving
    /// the eventual `<task-notification>` a fallback correlation key in case
    /// Claude Code strips `<tool-use-id>` from the notification body. Routed
    /// through the session's mailbox so the clear/upgrade is ordered after the
    /// `PreToolUse` that opened the running window.
    pub async fn on_post_tool_use(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_use_id: &str,
        tool_response_json: &str,
        transcript_path: &str,
    ) -> Result<Vec<SessionEvent>> {
        self.request(session_id, |reply| SessionInput::PostToolUse {
            tool_name: tool_name.to_owned(),
            tool_use_id: tool_use_id.to_owned(),
            tool_response_json: tool_response_json.to_owned(),
            transcript_path: transcript_path.to_owned(),
            reply,
        })
        .await
    }

    /// Handle a `PermissionRequest` hook: record the request row, register a
    /// decision waiter on the session's actor, and hand the transport the
    /// receiver it blocks on (with the `PermissionRequested` event to
    /// broadcast *before* blocking).
    ///
    /// The request-id → session index recorded here is what lets
    /// [`Self::decide_permission`] (which only knows the request id) route the
    /// browser's decision to the owning actor.
    pub async fn on_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        transcript_path: &str,
    ) -> Result<PermissionWait> {
        let wait = self
            .request(session_id, |reply| SessionInput::PermissionRequest {
                tool_name: tool_name.to_owned(),
                tool_input_json: tool_input_json.to_owned(),
                transcript_path: transcript_path.to_owned(),
                reply,
            })
            .await?;
        self.permission_index
            .lock()
            .expect("permission index poisoned")
            .insert(wait.request_id, session_id.clone());
        Ok(wait)
    }

    // ---- Permission decisions --------------------------------------------------

    /// Resolve a pending permission request with the browser's decision.
    ///
    /// Claims the request's index entry first (an atomic take, so two racing
    /// decisions cannot both win), then routes the decision to the owning
    /// session's actor, which wakes the blocked hook handler.
    ///
    /// Returns [`Error::PermissionNotPending`] when no waiter can be reached:
    /// the request is unknown, was already decided, or its hook wait timed
    /// out and fell back to the TUI prompt — in every case a UI decision can
    /// no longer take effect, and the caller surfaces that as a conflict.
    pub async fn decide_permission(
        &self,
        request_id: i64,
        decision: PermissionDecision,
    ) -> Result<Vec<SessionEvent>> {
        let session_id = self
            .permission_index
            .lock()
            .expect("permission index poisoned")
            .remove(&request_id)
            .ok_or(Error::PermissionNotPending(request_id))?;
        self.request(&session_id, |reply| SessionInput::DecidePermission {
            request_id,
            decision,
            reply,
        })
        .await
    }

    /// Abandon the waiter for a permission request whose hook wait timed out.
    ///
    /// The row stays `pending`: the hook responds with an empty passthrough,
    /// Claude Code shows its interactive TUI prompt, and the eventual
    /// `tool_result` resolves the row (see `sync_transcript`).
    pub async fn abandon_permission_decision(&self, request_id: i64) {
        let session_id = self
            .permission_index
            .lock()
            .expect("permission index poisoned")
            .remove(&request_id);
        if let Some(id) = session_id {
            self.sessions
                .post(&id, SessionInput::AbandonPermission { request_id });
        }
    }

    // ---- Question answers --------------------------------------------------------

    /// Answer a session's pending `AskUserQuestion` by injecting the selection
    /// keystrokes into its live TUI pane.
    ///
    /// Unlike a permission decision (keyed only by request id, so it needs the
    /// id→session index), a question answer carries the session id in its URL,
    /// so it routes straight to the owning actor. The actor correlates the
    /// `request_id` against its pending question, builds the pinned key sequence,
    /// and injects it via the tmux driver. `selections[q]` holds the chosen
    /// 0-based option indices for question `q`.
    ///
    /// Returns [`Error::QuestionNotPending`] (`409`) when no matching question
    /// is pending (already answered, stale, or no live pane), and
    /// [`Error::InvalidQuestionAnswer`] (`400`) for a malformed selection — in
    /// both cases the browser falls back to the terminal.
    pub async fn answer_question(
        &self,
        session_id: &SessionId,
        request_id: i64,
        selections: Vec<Vec<usize>>,
    ) -> Result<()> {
        self.request(session_id, |reply| SessionInput::AnswerQuestion {
            request_id,
            selections,
            reply,
        })
        .await
    }

    /// Cancel a session's pending `AskUserQuestion` by injecting `Escape` into
    /// its live TUI pane (a single Escape cancels the whole call).
    ///
    /// The sibling of [`answer_question`](Self::answer_question): like an answer,
    /// it carries the session id in its URL so it routes straight to the owning
    /// actor, which correlates the `request_id` against its pending question and
    /// injects the cancel keystroke via the tmux driver.
    ///
    /// Returns [`Error::QuestionNotPending`] (`409`) when no matching question is
    /// pending (already answered/cancelled, stale, or no live pane), in which
    /// case the browser falls back to the terminal. Unlike an answer there is no
    /// `400` case — cancel carries no selection to malform.
    pub async fn cancel_question(&self, session_id: &SessionId, request_id: i64) -> Result<()> {
        self.request(session_id, |reply| SessionInput::CancelQuestion {
            request_id,
            reply,
        })
        .await
    }

    // ---- Send cancellation -------------------------------------------------

    /// Cancel a `queued` send before it is dispatched, or a `dispatched` send
    /// whose echo has not arrived (typically the user pressed `Escape` in the
    /// TUI to discard the composer buffer, leaving no signal Delta can
    /// observe — see the module doc on
    /// [`cancel_send`](crate::interactor::cancel_send)).
    ///
    /// The cancel request carries only the send id (in its URL), so the
    /// owning session is derived from the send row here — mirroring how
    /// [`enqueue_send`](Self::enqueue_send) derives the session from a thread
    /// — and the cancel then executes on that session's actor, ordered
    /// against its dispatch path.
    ///
    /// Cancelling a `dispatched` send the turn machine is awaiting injects a
    /// single `Escape` into the pane (the same gesture
    /// [`cancel_question`](Self::cancel_question) uses) and promotes any
    /// queued send behind the cancelled head through the existing idle-flush
    /// path. A `dispatched` row the turn machine holds no claim on is
    /// cancelled as a pure state transition — no keystrokes, no turn input
    /// (see the module doc on ownerless rows).
    ///
    /// Returns [`Error::SendNotCancellable`] (`409`) when the send no longer
    /// exists, is already terminal (matched a transcript line, or already
    /// cancelled), or is `dispatched` but its echo has already arrived — the
    /// turn carries it `InFlight`, owned by its transcript line, and the
    /// user reaches for the in-flight interrupt instead. The browser drops
    /// its cancel control and reconciles from the next refetch on this
    /// error.
    pub async fn cancel_send(&self, send_id: i64) -> Result<()> {
        let Some(send) = self.store.send(send_id).await? else {
            return Err(Error::SendNotCancellable(send_id));
        };
        let session_id = send.session_id;
        self.request(&session_id, move |reply| SessionInput::CancelSend {
            send_id,
            reply,
        })
        .await
    }

    /// Release a *restored* send — one the boot-time reconcile recovered from
    /// a dead process's `dispatched` state — back into the normal queued
    /// flow (see the module doc on
    /// [`release_send`](crate::interactor::release_send)).
    ///
    /// Like a cancel, the release request carries only the send id (in its
    /// URL), so the owning session is derived from the send row here and the
    /// release then executes on that session's actor, ordered against its
    /// dispatch path. The actor first ensures the session is open — resuming
    /// it via `claude --resume <id>` when it is closed, the normal state
    /// right after the restart that created the restored row — exactly as an
    /// enqueue would. When the session was already open and idle the
    /// released row dispatches immediately through the normal queued path;
    /// the returned [`SessionEvent`]s (a `SendDispatched`, when that
    /// happened) are broadcast by the transport so the browser sees the
    /// transition. When the release itself resumed the session the row waits
    /// out the resume-readiness window and is typed by the resume-settle
    /// flush ([`Self::dispatch_ready_resumes`]).
    ///
    /// Returns [`Error::SendNotReleasable`] (`409`) when the send is
    /// unknown, was never restored, is already released, or has since been
    /// cancelled. The browser drops its Send control and reconciles from the
    /// next refetch on this error. An ensure-open failure — e.g.
    /// [`Error::ResumeUnavailable`] when the session's transcript is gone —
    /// surfaces as-is, before the restored marker is touched, so the release
    /// can be retried.
    pub async fn release_send(&self, send_id: i64) -> Result<Vec<SessionEvent>> {
        let Some(send) = self.store.send(send_id).await? else {
            return Err(Error::SendNotReleasable(send_id));
        };
        let session_id = send.session_id;
        let dispatched = self
            .request(&session_id, move |reply| SessionInput::ReleaseSend {
                send_id,
                reply,
            })
            .await?;
        Ok(dispatched.into_iter().collect())
    }

    // ---- Background ticks --------------------------------------------------------

    /// Poll the transcript of every currently-open (live-pane) session for
    /// newly-written lines, by fanning a sync tick out to every live actor.
    ///
    /// Drives the continuous background tail: Claude Code often flushes the
    /// final assistant line to the JSONL *after* the `Stop` hook fires, so the
    /// hook's sync misses it. The ticks are posted to all actors first and the
    /// replies awaited after, so the per-session syncs run **concurrently** —
    /// the old global sync lock serialized them; now only each session's own
    /// mailbox orders its ingestion. A session with no live pane no-ops.
    ///
    /// Each session that ingested new messages contributes one non-empty
    /// group, in arbitrary order — callers may index `group[0]` for the
    /// group's session id. Alongside the groups, returns any [`SessionEvent`]s
    /// the ingest produced (e.g. permission resolutions from a tailed-in
    /// `tool_result`, or `TurnInterrupted` from an interrupt marker).
    pub async fn poll_transcript(&self) -> Result<(Vec<Vec<Message>>, Vec<SessionEvent>)> {
        let mut pending = Vec::new();
        for id in self.sessions.ids() {
            let (tx, rx) = oneshot::channel();
            if self
                .sessions
                .post_existing(&id, SessionInput::SyncTick { reply: tx })
            {
                pending.push(rx);
            }
        }
        let mut groups = Vec::new();
        let mut events = Vec::new();
        for rx in pending {
            // A dropped reply means the actor retired mid-tick (tear-down);
            // skip it rather than failing the whole tick.
            let Ok(result) = rx.await else { continue };
            let (messages, session_events) = result?;
            events.extend(session_events);
            if !messages.is_empty() {
                groups.push(messages);
            }
        }
        Ok((groups, events))
    }

    /// Dispatch the held first prompt of every resume that is ready *and* has
    /// settled, on the background tick (see the `ResumeTick` input docs). A
    /// settled resume with no held prompt instead flushes its session's
    /// oldest genuinely `queued` send — the resume window defers queued
    /// dispatch, and this settle is what flushes it. Boot-restored sends are
    /// not flushed here; they wait for an explicit release
    /// ([`Self::release_send`]).
    ///
    /// Returns the [`SessionEvent::SendDispatched`]s those flushes produced,
    /// for the caller to broadcast so the browser sees each
    /// queued→dispatched transition.
    ///
    /// `now` is injected (rather than read here) so the dispatch is
    /// deterministic under test: the server loop passes `Instant::now()`,
    /// while tests advance a controlled instant.
    pub async fn dispatch_ready_resumes(&self, now: Instant) -> Result<Vec<SessionEvent>> {
        let mut pending = Vec::new();
        for id in self.sessions.ids() {
            let (tx, rx) = oneshot::channel();
            if self
                .sessions
                .post_existing(&id, SessionInput::ResumeTick { now, reply: tx })
            {
                pending.push(rx);
            }
        }
        let mut events = Vec::new();
        for rx in pending {
            let Ok(result) = rx.await else { continue };
            events.extend(result?);
        }
        Ok(events)
    }

    /// Reap launches that never became ready before their deadline (the
    /// watchdog sweep), covering both fresh spawns and resumed sessions.
    ///
    /// For each stale launch the owning actor kills the tmux pane
    /// (best-effort) and produces a [`SessionEvent::SpawnFailed`] so the
    /// browser can surface the failure and clear the optimistic pending chip.
    /// `now` is injected so the watchdog is deterministic under test; the
    /// server owns the periodic tick that calls this and broadcasts the
    /// result.
    pub async fn reap_stale_spawns(&self, now: Instant) -> Result<Vec<SessionEvent>> {
        let mut pending = Vec::new();
        for id in self.sessions.ids() {
            let (tx, rx) = oneshot::channel();
            if self
                .sessions
                .post_existing(&id, SessionInput::ReapTick { now, reply: tx })
            {
                pending.push(rx);
            }
        }
        let mut events = Vec::new();
        for rx in pending {
            let Ok(result) = rx.await else { continue };
            events.extend(result?);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod test_seams {
    //! Test-only seams over the per-session runtime state.
    //!
    //! These replace the lock-era seams that reached into the shared
    //! registries: each runs a closure inside the owning actor (in mailbox
    //! order, like any real input), so tests can seed launch state with
    //! controlled timestamps and read it back without widening the
    //! production surface.

    use std::time::Instant;

    use delta_model::SessionId;
    use tokio::sync::oneshot;

    use crate::interactor::session_actor::input::SessionInput;
    use crate::interactor::session_actor::runtime::{
        OpenHandle, PendingSpawn, ResumingSession, SessionRuntime,
    };
    use crate::interactor::Interactor;
    use crate::pane_token::PaneToken;
    use crate::ports::{pane_for, GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

    impl<T, X, S, W, G> Interactor<T, X, S, W, G>
    where
        T: TmuxDriver + 'static,
        X: Transcript + 'static,
        S: SessionStore + 'static,
        W: Workspace + 'static,
        G: GitWorktree + 'static,
    {
        /// Run a closure against a session's runtime state (spawning its actor
        /// if absent), returning the closure's result.
        pub(crate) async fn with_runtime<R: Send + 'static>(
            &self,
            id: &SessionId,
            f: impl FnOnce(&mut SessionRuntime) -> R + Send + 'static,
        ) -> R {
            let (tx, rx) = oneshot::channel();
            self.sessions.post(
                id,
                SessionInput::WithRuntime(Box::new(move |state| {
                    let _ = tx.send(f(state));
                })),
            );
            rx.await.expect("session actor dropped")
        }

        /// Like [`Self::with_runtime`], but never spawns an actor: returns
        /// `None` when the session has none (i.e. default runtime state).
        async fn with_runtime_existing<R: Send + 'static>(
            &self,
            id: &SessionId,
            f: impl FnOnce(&mut SessionRuntime) -> R + Send + 'static,
        ) -> Option<R> {
            let (tx, rx) = oneshot::channel();
            let posted = self.sessions.post_existing(
                id,
                SessionInput::WithRuntime(Box::new(move |state| {
                    let _ = tx.send(f(state));
                })),
            );
            if !posted {
                return None;
            }
            rx.await.ok()
        }

        /// The Delta-minted session ids of the currently-pending spawns, in
        /// spawn order.
        ///
        /// A fresh spawn's session id is a random UUID a test cannot predict,
        /// yet it is the hook-binding key. Tests spawn, read the id(s) back
        /// here, then fire a `UserPromptSubmit` carrying that exact id to
        /// bind. Spawn order is recovered from the pane token's monotonic
        /// mint ordinal (`delta-<n>`), since the pending entries now live one
        /// per actor.
        pub(crate) async fn pending_session_ids(&self) -> Vec<SessionId> {
            let mut found = Vec::new();
            for id in self.sessions.ids() {
                let ordinal = self
                    .with_runtime_existing(&id, |state| {
                        state
                            .pending_spawn()
                            .map(|spawn| token_ordinal(spawn.token.as_str()))
                    })
                    .await
                    .flatten();
                if let Some(ordinal) = ordinal {
                    found.push((ordinal, id));
                }
            }
            found.sort_by_key(|(ordinal, _)| *ordinal);
            found.into_iter().map(|(_, id)| id).collect()
        }

        /// Record a pending spawn with an explicit `created_at`, for watchdog
        /// tests.
        ///
        /// The production `created_at` is `Instant::now()` at spawn time,
        /// which a test cannot wind backwards. Reaper tests instead push a
        /// spawn stamped at a chosen instant (e.g. `now - 31s`) and then call
        /// `reap_stale_spawns(now)` so the deadline check is fully
        /// deterministic.
        pub(crate) async fn push_pending_spawn_at(
            &self,
            token: &str,
            session_id: &SessionId,
            created_at: Instant,
        ) {
            let token = token.to_owned();
            self.with_runtime(session_id, move |state| {
                state.push_pending(PendingSpawn {
                    token: PaneToken::from_raw(&token),
                    pane: pane_for(&token),
                    created_at,
                });
            })
            .await;
        }

        /// Bind a live, ready pane for a session, as if it had been spawned
        /// and become ready.
        ///
        /// Most enqueue/defer tests register `sess-1` then send to it, and
        /// want it to behave like a normal *open and ready* session (sends
        /// dispatch immediately). Registering via `on_user_prompt_submit`
        /// alone marks it known-but-closed, so the next send would resume it
        /// and — under the readiness gate — hold the first keystroke. This
        /// seam binds a ready pane up front so those tests exercise the
        /// immediate-dispatch path, not the resume gate (which has its own
        /// focused tests).
        pub(crate) async fn bind_open_session(&self, token: &str, session_id: &SessionId) {
            let token = token.to_owned();
            self.with_runtime(session_id, move |state| {
                state.bind(OpenHandle {
                    token: PaneToken::from_raw(&token),
                    pane: pane_for(&token),
                });
            })
            .await;
        }

        /// The session ids currently resuming-but-not-ready, for resume-gate
        /// tests.
        pub(crate) async fn resuming_session_ids(&self) -> Vec<SessionId> {
            let mut found = Vec::new();
            for id in self.sessions.ids() {
                let resuming = self
                    .with_runtime_existing(&id, |state| state.resuming().is_some())
                    .await
                    .unwrap_or(false);
                if resuming {
                    found.push(id);
                }
            }
            found
        }

        /// Apply a turn input directly to a session's state machine, for tests
        /// that seed a specific turn state (e.g. a held prompt's outstanding
        /// dispatch). State-only: the seeding transitions used by tests orphan
        /// nothing, so no store disposition runs here.
        pub(crate) async fn apply_turn_input(
            &self,
            id: &SessionId,
            input: crate::turn::TurnInput,
        ) -> crate::error::Result<crate::turn::TurnState> {
            Ok(self
                .with_runtime(id, move |state| state.apply_turn(input).next)
                .await)
        }

        /// Mark a resuming session ready at an explicit instant, for
        /// resume-dispatch tests. Returns whether the id was resuming (the
        /// production hook's return).
        pub(crate) async fn mark_resume_ready_at(&self, id: &SessionId, ready_at: Instant) -> bool {
            self.with_runtime(id, move |state| state.mark_resume_ready_at(ready_at))
                .await
        }

        /// Record a resuming (not-yet-ready) session with an explicit
        /// `created_at`, for resume-watchdog tests. A resuming session is
        /// also bound (its pane exists), so this mirrors production: bind the
        /// handle and record the resuming entry together.
        pub(crate) async fn push_resuming_at(
            &self,
            token: &str,
            session_id: &SessionId,
            held_prompt: Option<String>,
            created_at: Instant,
        ) {
            let token = token.to_owned();
            self.with_runtime(session_id, move |state| {
                state.bind(OpenHandle {
                    token: PaneToken::from_raw(&token),
                    pane: pane_for(&token),
                });
                state.start_resuming(ResumingSession {
                    token: PaneToken::from_raw(&token),
                    pane: pane_for(&token),
                    held_prompt,
                    created_at,
                    ready_at: None,
                });
            })
            .await;
        }
    }

    /// The numeric suffix of a minted `delta-<n>` token, for spawn ordering;
    /// raw test tokens without one sort first.
    fn token_ordinal(token: &str) -> u64 {
        token
            .rsplit('-')
            .next()
            .and_then(|suffix| suffix.parse().ok())
            .unwrap_or(0)
    }
}
