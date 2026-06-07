//! The [`Interactor`]: orchestrates the ports into Delta's use cases.

use delta_model::{Message, MessageUuid, PendingSend, Session, SessionId, Thread, ThreadId};

use crate::error::{Error, Result};
use crate::open_sessions::{OpenHandle, OpenSessions, PendingSpawn};
use crate::pane_token::{PaneToken, PaneTokenMinter};
use crate::ports::{
    pane_for, NewSession, SessionEvent, SessionLifecycle, SessionStore, StopHook, TmuxDriver,
    Transcript, UserPromptSubmitHook, Workspace,
};

/// The command Delta launches in each tmux session.
const SESSION_COMMAND: &str = "claude";

/// The `--resume` flag passed to `claude` to reattach to a stored conversation.
const RESUME_FLAG: &str = "--resume";

/// Holds the injected capabilities and exposes Delta's use cases.
///
/// Generic over the four ports so callers can inject any implementation. The
/// composition root and the application share a single concrete type through
/// the [`BoxedInteractor`] alias, which erases the gateways behind trait
/// objects; this keeps the transport layer's shared state non-generic while
/// still allowing tests to substitute fakes.
pub struct Interactor<T, X, S, W> {
    tmux: T,
    transcript: X,
    store: S,
    workspace: W,
    /// Base directory for per-spawn working directories.
    ///
    /// Each fresh spawn gets its own `<base>/<token>` subdirectory so the
    /// `cwd ↔ spawn` mapping is 1:1, making the hook-binding correlation exact.
    session_workdir_base: String,
    /// The Claude Code settings JSON written into each session's working
    /// directory so its hooks point back at this server. Rendered by the caller
    /// (with the running port) and held verbatim.
    session_settings_json: String,
    /// Mints unique [`PaneToken`]s for fresh spawns.
    minter: PaneTokenMinter,
    /// The in-memory registry of live (open) panes. Rebuilt empty on boot, so
    /// open/closed is process-runtime state and never persisted.
    open_sessions: tokio::sync::Mutex<OpenSessions>,
    /// Serializes [`Self::sync_transcript`] across callers.
    ///
    /// Both the hook handlers and the background transcript tail can sync
    /// concurrently. The read-cursor → read-file → ingest → set-cursor sequence
    /// is not atomic, so without this lock two interleaved syncs could read the
    /// same lines from the same starting cursor and double-ingest, or race the
    /// cursor write. Holding this for the whole sequence makes ingestion serial.
    sync_lock: tokio::sync::Mutex<()>,
}

/// An [`Interactor`] with its four ports type-erased behind trait objects.
///
/// Both the production composition root and integration tests build this exact
/// type, so the transport layer's shared state stays non-generic regardless of
/// which gateways are wired in.
pub type BoxedInteractor =
    Interactor<Box<dyn TmuxDriver>, Box<dyn Transcript>, Box<dyn SessionStore>, Box<dyn Workspace>>;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Construct an Interactor from the four injected ports plus the spawn
    /// configuration (the base working directory and rendered hook settings).
    pub fn new(
        tmux: T,
        transcript: X,
        store: S,
        workspace: W,
        session_workdir_base: impl Into<String>,
        session_settings_json: impl Into<String>,
    ) -> Self {
        Self {
            tmux,
            transcript,
            store,
            workspace,
            session_workdir_base: session_workdir_base.into(),
            session_settings_json: session_settings_json.into(),
            minter: PaneTokenMinter::new(),
            open_sessions: tokio::sync::Mutex::new(OpenSessions::default()),
            sync_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Borrow the store (useful for read-only queries from the transport layer).
    pub fn store(&self) -> &S {
        &self.store
    }

    /// The pane of a currently-open session, for the still-single PTY bridge.
    ///
    /// The single-session server drives at most one open pane, so this returns
    /// it (or `None` when nothing is open yet, so the PTY can fail cleanly rather
    /// than attach to a non-existent pane). The multi-session transport that
    /// lands next routes the PTY by session id via [`Self::pane_for_session`].
    pub async fn focused_pane(&self) -> Option<String> {
        self.open_sessions.lock().await.any_open_pane()
    }

    /// The pane driving a specific open session, if it is open.
    pub async fn pane_for_session(&self, id: &SessionId) -> Option<String> {
        self.open_sessions
            .lock()
            .await
            .handle(id)
            .map(|h| h.pane.clone())
    }

    #[cfg(test)]
    pub(crate) fn transcript(&self) -> &X {
        &self.transcript
    }

    #[cfg(test)]
    pub(crate) fn tmux(&self) -> &T {
        &self.tmux
    }

    #[cfg(test)]
    pub(crate) fn workspace(&self) -> &W {
        &self.workspace
    }

    /// Ensure at least one Claude Code session is up, spawning one if absent.
    ///
    /// Idempotent for the still-single server surface: if the registry already
    /// holds an open session (bound or pending) it is reused and
    /// [`SessionLifecycle::Ready`] is returned with no side effects. Otherwise a
    /// fresh session is spawned (see [`Self::new_session`]) and
    /// [`SessionLifecycle::Starting`] is returned.
    ///
    /// This drives only the tmux/process lifecycle. The conversational session
    /// is still registered later by the first `UserPromptSubmit` hook, so a
    /// freshly spawned session has no `Session` row yet — that is expected.
    pub async fn ensure_session(&self) -> Result<SessionLifecycle> {
        {
            let registry = self.open_sessions.lock().await;
            if registry.has_any_live() {
                return Ok(SessionLifecycle::Ready);
            }
        }
        self.new_session().await?;
        Ok(SessionLifecycle::Starting)
    }

    /// Spawn a fresh Claude Code session with no initial send (cold-start).
    ///
    /// Mints a token, prepares a unique `<base>/<token>` working directory with
    /// the hook settings written into it, launches `claude` in a new tmux
    /// session named after the token, and records a [`PendingSpawn`] (carrying
    /// no `first_prompt`). The conversational session id is learned later when
    /// the first `UserPromptSubmit` hook binds this spawn.
    pub async fn new_session(&self) -> Result<PaneToken> {
        self.spawn_fresh(None).await
    }

    /// Resume a closed but known session under a fresh tmux session.
    ///
    /// The conversational `session_id` is known up front, so this mints a fresh
    /// token, re-writes the hook settings into the session's stored `cwd` (the
    /// port is idempotent), launches `claude --resume <id>` there, and binds the
    /// new pane to `id` immediately. Resuming an already-open session is a no-op
    /// that returns the existing handle's token (the double-open guard).
    pub async fn open_session(&self, id: &SessionId) -> Result<PaneToken> {
        let session = self
            .store
            .session(id)
            .await?
            .ok_or_else(|| Error::SessionNotFound(id.as_str().to_owned()))?;

        let mut registry = self.open_sessions.lock().await;
        // Double-open guard: if already open, route to the existing pane.
        if let Some(handle) = registry.handle(id) {
            return Ok(handle.token.clone());
        }

        let token = self.minter.mint();
        let workdir = session.cwd.clone();
        // Settings must already be present from the original spawn, but re-write
        // them defensively in case the port is fresh or the file was lost.
        self.workspace
            .write_session_settings(&workdir, &self.session_settings_json)
            .await?;
        let command = vec![
            SESSION_COMMAND.to_owned(),
            RESUME_FLAG.to_owned(),
            id.as_str().to_owned(),
        ];
        self.tmux
            .create_session(token.as_str(), &workdir, &command)
            .await?;
        let pane = pane_for(token.as_str());
        registry.bind(
            id.clone(),
            OpenHandle {
                token: token.clone(),
                pane,
                workdir,
            },
        );
        Ok(token)
    }

    /// Close an open session: kill its pane and drop it from the registry.
    ///
    /// The conversational data remains in the store; only the live pane and the
    /// `claude` process are torn down. A session that is not open is a no-op.
    pub async fn close_session(&self, id: &SessionId) -> Result<()> {
        let handle = {
            let mut registry = self.open_sessions.lock().await;
            registry.remove(id)
        };
        if let Some(handle) = handle {
            self.tmux.kill_session(handle.token.as_str()).await?;
        }
        Ok(())
    }

    /// Spawn a fresh session, optionally deferring a first send.
    ///
    /// Mints a token, creates the unique `<base>/<token>` workdir with settings
    /// written, launches `claude` there, and records a [`PendingSpawn`] carrying
    /// `first_prompt`. Returns the minted token.
    async fn spawn_fresh(&self, first_prompt: Option<String>) -> Result<PaneToken> {
        // Mint and record the pending spawn under the registry lock so the
        // token counter and the pending list advance together.
        let mut registry = self.open_sessions.lock().await;
        let token = self.minter.mint();
        let workdir = self.workdir_for(&token);
        let pane = pane_for(token.as_str());

        self.workspace
            .write_session_settings(&workdir, &self.session_settings_json)
            .await?;
        let command = vec![SESSION_COMMAND.to_owned()];
        self.tmux
            .create_session(token.as_str(), &workdir, &command)
            .await?;

        registry.push_pending(PendingSpawn {
            token: token.clone(),
            pane,
            workdir,
            first_prompt,
        });
        Ok(token)
    }

    /// The unique working directory for a spawn: `<base>/<token>`.
    fn workdir_for(&self, token: &PaneToken) -> String {
        std::path::Path::new(&self.session_workdir_base)
            .join(token.as_str())
            .to_string_lossy()
            .into_owned()
    }

    /// Enqueue a user input to be sent into the session, ensuring it is open.
    ///
    /// The send target is resolved to the single session (temporary shim). The
    /// session is then ensured open before the text is dispatched:
    ///
    /// - **Open** (a live pane is bound): the text is dispatched immediately on
    ///   the normal path — the `pending_send` row is written *before* the
    ///   keystrokes, so the correlation head is in place when the
    ///   `UserPromptSubmit` hook fires, with the cancel-on-dispatch-failure
    ///   rollback below.
    /// - **Closed** (a session exists in the store but no live pane): it is
    ///   resumed via [`Self::open_session`] (`claude --resume <id>`), then the
    ///   normal path runs.
    /// - **None** (no session yet — a composer-first message): a fresh session
    ///   is spawned with the text deferred as its `first_prompt`. The
    ///   `pending_send` row cannot be written yet (it references a session id
    ///   that does not exist), so it is held on the spawn and written when the
    ///   first `UserPromptSubmit` binds the spawn. A synthetic, not-yet-persisted
    ///   [`PendingSend`] is returned so the REST surface has a response.
    ///
    /// If `branch_from` is set, this is the first message of a new branch: an
    /// unnamed child thread is created off that message and the send is
    /// attributed to it. A branch send requires an existing session (there must
    /// be a message to branch from), so it is rejected when there is no session.
    pub async fn enqueue_send(
        &self,
        thread_id: ThreadId,
        text: &str,
        locator_quote: Option<&str>,
        branch_from: Option<&MessageUuid>,
    ) -> Result<PendingSend> {
        match self.resolve_single_session().await? {
            Some(session) => {
                // Ensure the resolved session is open: resume it if it is a known
                // but closed session (no live pane). Once open we have a pane to
                // dispatch to and the normal pre-dispatch path applies.
                let pane = self.ensure_open(&session.id).await?;
                self.enqueue_into_open(
                    &session.id,
                    &pane,
                    thread_id,
                    text,
                    locator_quote,
                    branch_from,
                )
                .await
            }
            None => {
                // No session at all: a composer-first message. There is nothing
                // to branch from yet, so reject branch sends.
                if branch_from.is_some() {
                    return Err(Error::NoSession);
                }
                self.spawn_fresh(Some(text.to_owned())).await?;
                Ok(deferred_pending_send(thread_id, text, locator_quote))
            }
        }
    }

    /// Write the `pending_send` row and dispatch the keystrokes for an open
    /// session, with the cancel-on-dispatch-failure rollback.
    #[allow(clippy::too_many_arguments)]
    async fn enqueue_into_open(
        &self,
        session_id: &SessionId,
        pane: &str,
        thread_id: ThreadId,
        text: &str,
        locator_quote: Option<&str>,
        branch_from: Option<&MessageUuid>,
    ) -> Result<PendingSend> {
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
                    .create_thread(session_id, &title, Some(thread_id), Some(parent))
                    .await?;
                (thread.id, Some(parent.clone()))
            }
            None => (thread_id, None),
        };

        let pending = self
            .store
            .enqueue_send(
                session_id,
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
        if let Err(dispatch_err) = self.tmux.send_line(pane, text).await {
            let _ = self.store.cancel_send(pending.id).await;
            return Err(dispatch_err);
        }
        Ok(pending)
    }

    /// Ensure a known session is open, returning the pane to dispatch into.
    ///
    /// If it is already open the existing pane is returned; otherwise it is
    /// resumed via [`Self::open_session`] and the freshly-bound pane is returned.
    async fn ensure_open(&self, id: &SessionId) -> Result<String> {
        {
            let registry = self.open_sessions.lock().await;
            if let Some(handle) = registry.handle(id) {
                return Ok(handle.pane.clone());
            }
        }
        // Not open: resume it. `open_session` binds the new pane under the lock.
        self.open_session(id).await?;
        let registry = self.open_sessions.lock().await;
        registry
            .handle(id)
            .map(|h| h.pane.clone())
            .ok_or_else(|| Error::SessionNotFound(id.as_str().to_owned()))
    }

    /// Create a named branch off an existing message without sending anything.
    ///
    /// Temporary single-session shim: targets the single session via
    /// [`Self::resolve_single_session`]. Remove when the multi-session REST
    /// surface lands.
    pub async fn create_branch(
        &self,
        parent_thread_id: ThreadId,
        root_message_uuid: &MessageUuid,
        title: &str,
    ) -> Result<Thread> {
        let session = self
            .resolve_single_session()
            .await?
            .ok_or(Error::NoSession)?;
        self.store
            .create_thread(
                &session.id,
                title,
                Some(parent_thread_id),
                Some(root_message_uuid),
            )
            .await
    }

    /// Register a session on the first `UserPromptSubmit` for its id, binding it
    /// to a fresh spawn when one is waiting.
    ///
    /// A hook's `session_id` is unknown the first time Claude Code reports it.
    /// Two cases are distinguished by the hook's `cwd`:
    ///
    /// - **Fresh spawn binding**: a [`PendingSpawn`] whose unique workdir equals
    ///   `cwd` is moved `pending → bound[session_id]`. The session row is
    ///   registered, and if the spawn carried a deferred `first_prompt` (a
    ///   composer-initiated New), the held `pending_send` is written *now* —
    ///   with the now-known session id — *before* the caller's
    ///   `match_pending_send` runs, so the first prompt correlates through the
    ///   normal FIFO machinery.
    /// - **External claude**: no pending spawn matches `cwd`, so this is a
    ///   `claude` started outside Delta. The session is registered as a
    ///   known-but-closed data session (no [`OpenHandle`]) and a warning is
    ///   logged, preserving today's external-input behaviour.
    async fn register_on_first_contact(
        &self,
        hook: &UserPromptSubmitHook,
        events: &mut Vec<SessionEvent>,
    ) -> Result<Session> {
        // Match a waiting spawn by workdir under the registry lock, taking its
        // deferred first prompt with it.
        let bound = {
            let mut registry = self.open_sessions.lock().await;
            match registry.take_pending_for_workdir(&hook.cwd) {
                Some(spawn) => {
                    registry.bind(
                        hook.session_id.clone(),
                        OpenHandle {
                            token: spawn.token,
                            pane: spawn.pane,
                            workdir: spawn.workdir,
                        },
                    );
                    spawn.first_prompt
                }
                None => {
                    tracing::warn!(
                        session_id = %hook.session_id,
                        cwd = %hook.cwd,
                        "UserPromptSubmit for an unknown session with no matching pending spawn; \
                         registering as an external, closed data session"
                    );
                    return self
                        .register_session_row(hook, events)
                        .await
                        .map(|(s, _)| s);
                }
            }
        };

        let (session, main_id) = self.register_session_row(hook, events).await?;

        // Write the deferred first send now that the session id is known, so the
        // caller's `match_pending_send` finds it and the first prompt correlates
        // through the normal machinery. The text is sent into the pane up front
        // by the spawn's keystroke dispatch, so this only writes the FIFO head.
        if let Some(text) = bound {
            self.store
                .enqueue_send(&session.id, main_id, None, &text, None)
                .await?;
        }
        Ok(session)
    }

    /// Insert the session row and emit [`SessionEvent::SessionRegistered`],
    /// returning the session and its `main` thread id.
    async fn register_session_row(
        &self,
        hook: &UserPromptSubmitHook,
        events: &mut Vec<SessionEvent>,
    ) -> Result<(Session, ThreadId)> {
        let (session, main_id) = self
            .store
            .register_session(NewSession {
                id: hook.session_id.clone(),
                cwd: hook.cwd.clone(),
                transcript_path: hook.transcript_path.clone(),
            })
            .await?;
        events.push(SessionEvent::SessionRegistered {
            session_id: hook.session_id.clone(),
        });
        Ok((session, main_id))
    }

    /// Handle a `UserPromptSubmit` hook.
    ///
    /// The first hook for a given `session_id` registers that session
    /// (SessionStart never fires); routing by id lets several Claude Code
    /// sessions register independently.
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

        // Register on first contact for THIS session id (Claude Code never fires
        // SessionStart). Routing by id lets several Claude Code sessions register
        // independently rather than assuming a single global one.
        let session = match self.store.session(&hook.session_id).await? {
            Some(session) => session,
            None => self.register_on_first_contact(&hook, &mut events).await?,
        };

        // Resolve this prompt's queued send *before* syncing, so the locator
        // quote is returned as `additionalContext` even when the user line has
        // not been ingested yet (the common timing case). Match by text — not by
        // FIFO head — so a stale send stuck at the head cannot suppress the quote
        // or misfire external-input detection.
        let pending = self
            .store
            .match_pending_send(&hook.session_id, hook.prompt.trim())
            .await?;
        let additional_context = pending
            .as_ref()
            .and_then(|p| p.locator_quote.as_deref())
            .and_then(frame_locator_context);

        // Ingest new transcript lines. This matches each user line to its queued
        // send and attributes it (plus the assistant lines that follow it) to
        // the right thread, marking the send matched as a side effect.
        let new_messages = self.sync_transcript(&session).await?;

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
        // Route by the hook's own session id so the right session's transcript is
        // synced, even when several sessions are registered.
        if let Some(session) = self.store.session(&hook.session_id).await? {
            self.sync_transcript(&session).await?;
        }
        Ok(vec![SessionEvent::TurnCompleted {
            session_id: hook.session_id,
            stop_reason: hook.stop_reason,
        }])
    }

    /// Poll every registered session's transcript for newly-written lines.
    ///
    /// Drives the continuous background tail: Claude Code often flushes the final
    /// assistant line to the JSONL *after* the `Stop` hook fires, so the hook's
    /// sync misses it and the reply never reaches the browser until the next
    /// hook. Polling on an interval ingests those late lines and returns them so
    /// the caller can announce the transcript growth.
    ///
    /// Each session is synced independently and the result is grouped by session:
    /// one entry per session that ingested new messages, in registration order.
    /// A closed or quiet session simply yields no new lines and is omitted, so
    /// every returned group is non-empty — callers may index `group[0]` for the
    /// group's session id. This lets the caller emit one transcript-growth
    /// notification per session.
    ///
    /// Reuses [`Self::sync_transcript`] (cursor, attribution, the serialization
    /// lock), so it is safe to call concurrently with the hook handlers. Returns
    /// an empty list when no session has been registered yet.
    pub async fn poll_transcript(&self) -> Result<Vec<Vec<Message>>> {
        let mut groups = Vec::new();
        for session in self.store.list_sessions().await? {
            let messages = self.sync_transcript(&session).await?;
            if !messages.is_empty() {
                groups.push(messages);
            }
        }
        Ok(groups)
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
    /// Returns `None` until the first `UserPromptSubmit` hook registers a
    /// session (Claude Code never fires `SessionStart`).
    ///
    /// Temporary single-session shim: resolves to the single session via
    /// [`Self::resolve_single_session`]. Remove when the multi-session REST
    /// surface lands.
    pub async fn current_session(&self) -> Result<Option<(Session, ThreadId)>> {
        match self.resolve_single_session().await? {
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
    ///
    /// Temporary single-session shim: resolves to the single session via
    /// [`Self::resolve_single_session`]. Remove when the multi-session REST
    /// surface lands.
    pub async fn threads(&self) -> Result<Vec<Thread>> {
        match self.resolve_single_session().await? {
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
    async fn sync_transcript(&self, session: &Session) -> Result<Vec<Message>> {
        // Serialize the whole cursor → read → ingest → cursor sequence so the
        // hook handlers and the background tail cannot interleave and
        // double-ingest or race the cursor (see `sync_lock`).
        let _guard = self.sync_lock.lock().await;

        let transcript_path = &session.transcript_path;
        let main_thread = self.store.main_thread_id(&session.id).await?;

        // Resume from the line-based cursor so each transcript line is read
        // exactly once. This is the file line index, not a message count: lines
        // that parse to nothing (blank, no-uuid such as Claude Code's
        // `file-history-snapshot`, or unparsable) still advance it, so the
        // cursor never lags behind the file and already-ingested lines are never
        // reprocessed.
        let from = self.store.transcript_lines_read(&session.id).await?;
        let read = self.transcript.read_from(transcript_path, from).await?;

        // Always advance the cursor to the file's true line count, even when no
        // new messages parsed, so skipped trailing lines are not re-read next
        // time.
        self.store
            .set_transcript_lines_read(&session.id, read.total_lines)
            .await?;

        if read.messages.is_empty() {
            return Ok(Vec::new());
        }

        // The turn in progress when this batch starts: the thread of the most
        // recent persisted user message, defaulting to `main`.
        let mut carry_thread = self
            .store
            .latest_user_thread(&session.id)
            .await?
            .unwrap_or(main_thread);

        let mut messages = Vec::with_capacity(read.messages.len());
        for line in read.messages {
            let content_text = Message::flatten_text(&line.content);

            let (thread_id, semantic_parent_uuid) = if matches!(line.role, delta_model::Role::User)
            {
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
                // Persist the message's own transcript line index as its `seq`,
                // so ordering follows true file position with no drift.
                seq: line.seq,
                content_text,
                content: line.content,
                created_at: line.created_at.unwrap_or_default(),
            });
        }

        self.store.upsert_messages(&messages).await?;
        Ok(messages)
    }

    /// Resolve "the" session for the still-single-session server surface.
    ///
    /// Temporary single-session shim: the browser-facing REST API
    /// (`GET /api/session`, `GET /api/threads`, `POST /api/sends`) is still
    /// single-session, so the methods backing it collapse the multi-session
    /// store down to one session — the most-recently-created one — preserving
    /// today's observable behaviour. Returns `None` when no session exists yet.
    /// Remove this once the multi-session REST surface routes by session id.
    async fn resolve_single_session(&self) -> Result<Option<Session>> {
        Ok(self.store.list_sessions().await?.into_iter().last())
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

/// Build the synthetic, not-yet-persisted [`PendingSend`] returned for a
/// composer-first send that spawned a fresh session.
///
/// No `pending_send` row exists yet — it references a session id that does not
/// exist until the first `UserPromptSubmit` binds the spawn. This shapes a
/// response for the REST surface meanwhile: `id` is `0` (no row), the status is
/// `Pending`, and the session id is left empty. The real, correlatable row is
/// written at bind time.
fn deferred_pending_send(
    thread_id: ThreadId,
    text: &str,
    locator_quote: Option<&str>,
) -> PendingSend {
    PendingSend {
        id: 0,
        session_id: SessionId::from(""),
        thread_id,
        semantic_parent_uuid: None,
        text: text.to_owned(),
        locator_quote: locator_quote.map(str::to_owned),
        status: delta_model::PendingSendStatus::Pending,
        matched_uuid: None,
        created_at: String::new(),
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

/// Frame a locator quote for injection as `additionalContext`.
///
/// The locator quote is a passage the user selected from earlier in the
/// conversation to anchor their current message. Injecting it verbatim gives
/// the model no provenance, so it may read the bare text as new content or a
/// fresh instruction. This wraps it in a short frame that supplies that missing
/// provenance, with the quote delimited so the frame and the quote stay
/// distinguishable.
///
/// The frame is authorship-neutral: the selected passage may come from either an
/// assistant or a user message, so it does not claim who said it. An empty or
/// whitespace-only quote carries no content to anchor, so it yields `None` and
/// nothing is injected.
///
/// Isolated deliberately so the exact wording is easy to tune. This affects only
/// the model-facing `additionalContext`; it never changes the on-screen message
/// or any stored field.
fn frame_locator_context(quote: &str) -> Option<String> {
    let quote = quote.trim();
    if quote.is_empty() {
        return None;
    }
    Some(format!(
        "The user is replying to this passage they selected from earlier in the conversation:\n\"{quote}\""
    ))
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
