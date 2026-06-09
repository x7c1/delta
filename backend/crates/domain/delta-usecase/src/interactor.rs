//! The [`Interactor`]: orchestrates the ports into Delta's use cases.

use delta_model::{Message, MessageUuid, PendingSend, Session, SessionId, Thread, ThreadId};

use crate::error::{Error, Result};
use crate::open_sessions::{OpenHandle, OpenSessions, PendingSpawn};
use crate::pane_token::{PaneToken, PaneTokenMinter};
use crate::ports::{
    pane_for, DirListing, NewSession, RecentWorkdir, SessionEvent, SessionLifecycle, SessionStore,
    StopHook, TmuxDriver, Transcript, UserPromptSubmitHook, Workspace,
};
use crate::send_target::SendTarget;
use crate::session_listing::SessionListing;
use crate::session_page::{SessionPage, SessionPageCursor};

/// The command Delta launches in each tmux session.
const SESSION_COMMAND: &str = "claude";

/// How many recently-used working directories the picker's "recent" list returns.
const RECENT_WORKDIRS_LIMIT: u32 = 20;

/// The `--resume` flag passed to `claude` to reattach to a stored conversation.
const RESUME_FLAG: &str = "--resume";

/// The `--session-id` flag passed to `claude` to pin a fresh conversation's
/// `session_id` to a value Delta mints up front. With the id known at spawn
/// time, the first `UserPromptSubmit` hook reports exactly that id, so a fresh
/// spawn correlates to its session by id — never by working directory.
const SESSION_ID_FLAG: &str = "--session-id";

/// The `--settings` flag passed to `claude` to load Delta's session settings
/// (hooks + theme) from a Delta-owned file, instead of writing them into the
/// session's working directory and risking a clobber of a real project's
/// `.claude/settings.json`.
const SETTINGS_FLAG: &str = "--settings";

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
    /// Each fresh spawn runs in its own `<base>/<token>` subdirectory. The
    /// workdir is no longer the hook-binding key — correlation is by the
    /// Delta-minted session id pinned via `claude --session-id` — so this base
    /// is free to become a user-selected project path in a later change without
    /// breaking spawn↔session correlation.
    session_workdir_base: String,
    /// The Claude Code settings JSON whose hooks point back at this server (and
    /// which pins the session theme). Rendered by the caller (with the running
    /// port) and held verbatim; written to [`Self::session_settings_path`] and
    /// passed to `claude --settings`.
    session_settings_json: String,
    /// Delta-owned path the settings JSON is written to before each launch, then
    /// passed to `claude --settings <path>`. Kept *outside* any session working
    /// directory so spawning/resuming in a real project never overwrites that
    /// project's own `.claude/settings.json`.
    session_settings_path: String,
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
    /// configuration (the base working directory, the rendered settings JSON,
    /// and the Delta-owned path that JSON is written to for `--settings`).
    pub fn new(
        tmux: T,
        transcript: X,
        store: S,
        workspace: W,
        session_workdir_base: impl Into<String>,
        session_settings_json: impl Into<String>,
        session_settings_path: impl Into<String>,
    ) -> Self {
        Self {
            tmux,
            transcript,
            store,
            workspace,
            session_workdir_base: session_workdir_base.into(),
            session_settings_json: session_settings_json.into(),
            session_settings_path: session_settings_path.into(),
            minter: PaneTokenMinter::new(),
            open_sessions: tokio::sync::Mutex::new(OpenSessions::default()),
            sync_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Borrow the store (useful for read-only queries from the transport layer).
    pub fn store(&self) -> &S {
        &self.store
    }

    /// The pane driving a specific open session, if it is open.
    ///
    /// The PTY bridge routes by session id: with no live pane for `id` this
    /// returns `None` so the bridge can refuse the attach rather than bind to a
    /// non-existent pane.
    pub async fn pane_for_session(&self, id: &SessionId) -> Option<String> {
        self.open_sessions
            .lock()
            .await
            .handle(id)
            .map(|h| h.pane.clone())
    }

    /// Wipe the residual input of a session's pane, if it is open.
    ///
    /// Resolves the session's live pane from the registry exactly like
    /// [`Self::pane_for_session`] does. When the session is open the pane's
    /// current input is cleared via the driver; when it is not open there is no
    /// live pane to clear, so this is a no-op returning `Ok(())`.
    ///
    /// Intended for use right before a fresh PTY attach: a prior client's detach
    /// leaves a focus-out (`ESC[O`) that Claude renders as a stray blank line, so
    /// clearing on the next attach keeps the input box clean across reconnects.
    pub async fn clear_session_input(&self, id: &SessionId) -> Result<()> {
        let pane = self
            .open_sessions
            .lock()
            .await
            .handle(id)
            .map(|h| h.pane.clone());
        if let Some(pane) = pane {
            self.tmux.clear_input(&pane).await?;
        }
        Ok(())
    }

    /// Whether a session is currently open (driven by a live pane).
    ///
    /// Open/closed is process-runtime state held by the registry, so this is the
    /// authority the session-list endpoint annotates each stored session with.
    pub async fn is_session_open(&self, id: &SessionId) -> bool {
        self.open_sessions.lock().await.is_open(id)
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

    /// The Delta-minted session ids of the currently-pending spawns, in order.
    ///
    /// Test-only seam: a fresh spawn's session id is a random UUID a test cannot
    /// predict, yet it is now the hook-binding key. Tests spawn, read the id(s)
    /// back here, then fire a `UserPromptSubmit` carrying that exact id to bind.
    #[cfg(test)]
    pub(crate) async fn pending_session_ids(&self) -> Vec<SessionId> {
        self.open_sessions.lock().await.pending_session_ids()
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
        self.spawn_fresh(None, None).await
    }

    /// Resume a closed but known session under a fresh tmux session.
    ///
    /// The conversational `session_id` is known up front, so this mints a fresh
    /// token, re-writes the settings file (at Delta's own path, not the session
    /// cwd; the port is idempotent), launches `claude --settings <file> --resume
    /// <id>` in the stored cwd, and binds the new pane to `id` immediately.
    /// Resuming an already-open session is a no-op
    /// that returns the existing handle's token (the double-open guard).
    ///
    /// Before returning, the existing transcript is synced so the DB's message
    /// rows and read cursor catch up to whatever Claude Code already wrote for
    /// this conversation. This matters because the resume's first
    /// `UserPromptSubmit` resolves thread context from already-persisted history:
    /// [`Self::thread_switch_context`] reads [`SessionStore::latest_user_thread`]
    /// and [`Self::sync_transcript`] seeds `carry_thread` from it. If the DB were
    /// behind the transcript at that first prompt (a cold/just-restored DB, or
    /// any DB-behind-transcript state), `latest_user_thread` would report `None`,
    /// mis-seeding `carry_thread` to `main` and mis-attributing any leading
    /// non-user line of the resumed batch. Catching up here, before
    /// `claude --resume` can produce a new prompt hook, makes the user's actual
    /// last thread visible on that first prompt. The sync is `sync_lock`-guarded
    /// and cursor-based idempotent, so it never double-ingests.
    pub async fn open_session(&self, id: &SessionId) -> Result<PaneToken> {
        let session = self
            .store
            .session(id)
            .await?
            .ok_or_else(|| Error::SessionNotFound(id.as_str().to_owned()))?;

        let token = {
            let mut registry = self.open_sessions.lock().await;
            // Double-open guard: if already open, route to the existing pane.
            if let Some(handle) = registry.handle(id) {
                return Ok(handle.token.clone());
            }

            // Resume gate: `claude --resume <id>` replays from the local JSONL
            // transcript, so a missing transcript makes resume impossible. tmux
            // would still report a clean spawn (it only checks `new-session`'s
            // exit code, which is 0 before claude's own resume failure surfaces),
            // leaving the UI stuck on a "waiting" pending row that never clears.
            // Refuse here — before minting a token, writing settings, or spawning
            // — so no pane is created and no optimistic pending send is enqueued.
            if !self.transcript.exists(&session.transcript_path).await? {
                return Err(Error::ResumeUnavailable(id.as_str().to_owned()));
            }

            let token = self.mint_free_token().await?;
            let workdir = session.cwd.clone();
            // Re-write the settings file before resuming, in case the port is
            // fresh or the file was lost. It lives at a Delta-owned path, not in
            // `workdir`, so resuming in a real project never touches that
            // project's own `.claude/settings.json`.
            self.workspace
                .write_session_settings(&self.session_settings_path, &self.session_settings_json)
                .await?;
            let command = vec![
                SESSION_COMMAND.to_owned(),
                SETTINGS_FLAG.to_owned(),
                self.session_settings_path.clone(),
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
            token
        };

        // Catch the DB up to the existing transcript before the resume's first
        // prompt can arrive, so thread context resolves against the user's real
        // last thread rather than a DB-behind `None`. Released the registry lock
        // above first: `sync_transcript` takes its own `sync_lock` and does not
        // need the registry held.
        self.sync_transcript(&session).await?;
        Ok(token)
    }

    /// Close an open session: kill its pane and drop it from the registry.
    ///
    /// The conversational data remains in the store; only the live pane and the
    /// `claude` process are torn down. Closing a known session that is not open
    /// is a no-op (it has no live pane to tear down), but an *unknown* id is a
    /// clean `SessionNotFound` (404) — the same rejection [`Self::open_session`]
    /// gives — so the browser can tell "already closed" apart from "no such
    /// session" rather than having a stale id silently succeed.
    pub async fn close_session(&self, id: &SessionId) -> Result<()> {
        if self.store.session(id).await?.is_none() {
            return Err(Error::SessionNotFound(id.as_str().to_owned()));
        }
        let handle = {
            let mut registry = self.open_sessions.lock().await;
            registry.remove(id)
        };
        if let Some(handle) = handle {
            self.tmux.kill_session(handle.token.as_str()).await?;
        }
        Ok(())
    }

    /// Spawn a fresh session, optionally dispatching a first prompt.
    ///
    /// Mints a token and a fresh Claude `session_id` (a time-ordered UUID v7, so
    /// session ids sort chronologically by creation time), launches
    /// `claude --settings <path> --session-id <uuid>` in the launch directory,
    /// and records a
    /// [`PendingSpawn`] carrying that minted id (the binding key) and
    /// `first_prompt`. Pinning the id up front means the first `UserPromptSubmit`
    /// hook reports exactly this id, so the spawn correlates to its session by id
    /// rather than by working directory. When a `first_prompt` is present (a
    /// composer-initiated New), it is typed into the freshly-created pane so
    /// Claude actually receives the message and fires the `UserPromptSubmit` hook
    /// that binds this spawn — the hook then writes the deferred `pending_send`
    /// row that lets the first user line correlate. Returns the minted token.
    ///
    /// The registry lock is taken only for the brief record/rollback steps, never
    /// across the tmux/workspace I/O (which includes the create-session settle
    /// delay), so a spawn does not serialize concurrent registry readers (hooks,
    /// the PTY bridge) for the whole spawn duration. The `PendingSpawn` is
    /// recorded *before* the first prompt is dispatched, so the
    /// `UserPromptSubmit` that prompt triggers always finds a spawn to bind
    /// rather than racing ahead and being misread as external input.
    ///
    /// When `workdir` is `Some`, it is a user-selected path: it is validated and
    /// canonicalized via [`Workspace::resolve_existing_dir`] *before* anything is
    /// minted or launched, so an invalid path fails cleanly with no token, no
    /// pane, and no pending spawn left behind (mirroring the resume gate in
    /// [`Self::open_session`]). When `None`, the spawn falls back to its default
    /// per-token `<base>/<token>` directory.
    async fn spawn_fresh(
        &self,
        first_prompt: Option<String>,
        workdir: Option<String>,
    ) -> Result<PaneToken> {
        // Validate a user-selected workdir before minting or launching anything,
        // so an invalid path is rejected with no side effects. The canonical
        // path becomes the launch directory; `None` defers to `<base>/<token>`
        // computed after the token is minted, below.
        let requested_workdir = match workdir {
            Some(dir) => Some(self.workspace.resolve_existing_dir(&dir).await?),
            None => None,
        };

        // The minter is atomic, so token uniqueness needs no lock here.
        let token = self.mint_free_token().await?;
        let workdir = requested_workdir.unwrap_or_else(|| self.workdir_for(&token));
        let pane = pane_for(token.as_str());

        // Mint and pin the conversation's session id up front. `claude
        // --session-id <uuid>` makes the first `UserPromptSubmit` hook report
        // exactly this id, so the spawn correlates to its session by id rather
        // than by working directory. The id is a time-ordered UUID v7 (a 48-bit
        // millisecond timestamp prefix followed by random bits), so session ids
        // sort chronologically by creation time while remaining a fully valid
        // RFC 9562 UUID, and collision with an existing stored session is
        // astronomically unlikely.
        let session_id = SessionId::from(uuid::Uuid::now_v7().to_string());

        self.workspace
            .write_session_settings(&self.session_settings_path, &self.session_settings_json)
            .await?;
        let command = vec![
            SESSION_COMMAND.to_owned(),
            SETTINGS_FLAG.to_owned(),
            self.session_settings_path.clone(),
            SESSION_ID_FLAG.to_owned(),
            session_id.as_str().to_owned(),
        ];
        self.tmux
            .create_session(token.as_str(), &workdir, &command)
            .await?;

        // Record the spawn before dispatching the first prompt, so the hook the
        // prompt triggers can bind it. (A failed create above returns early with
        // nothing recorded, so no dangling pending spawn is left behind.)
        self.open_sessions.lock().await.push_pending(PendingSpawn {
            token: token.clone(),
            pane: pane.clone(),
            session_id,
            workdir,
            first_prompt: first_prompt.clone(),
        });

        // Type the deferred first prompt into the new pane. If it never reaches
        // the pane the spawn would sit idle forever (Claude never fires the hook
        // that binds it), so roll the pending spawn back and surface the error.
        if let Some(text) = first_prompt {
            if let Err(dispatch_err) = self.tmux.send_line(&pane, &text).await {
                self.open_sessions
                    .lock()
                    .await
                    .remove_pending_for_token(&token);
                return Err(dispatch_err);
            }
        }
        Ok(token)
    }

    /// Mint a pane token whose tmux session name is not already in use.
    ///
    /// The minter's counter resets on each server start, but `delta-<n>` tmux
    /// sessions from a previous run can survive a restart — they are detached,
    /// so stopping the server does not kill them. Creating a tmux session with a
    /// name that already exists fails with "duplicate session", which would 500
    /// a spawn. So skip any minted name whose tmux session is still alive and
    /// advance to the next free one. The monotonic counter guarantees this
    /// terminates (there are finitely many surviving sessions) and that two
    /// concurrent spawns never contend for the same name.
    async fn mint_free_token(&self) -> Result<PaneToken> {
        loop {
            let token = self.minter.mint();
            if !self.tmux.has_session(token.as_str()).await? {
                return Ok(token);
            }
        }
    }

    /// The working directory for a spawn: `<base>/<token>`.
    ///
    /// Distinct per spawn today, but no longer required to be: correlation is by
    /// the Delta-minted session id, not the workdir.
    fn workdir_for(&self, token: &PaneToken) -> String {
        std::path::Path::new(&self.session_workdir_base)
            .join(token.as_str())
            .to_string_lossy()
            .into_owned()
    }

    /// Enqueue a user input, routing it to the session the target names.
    ///
    /// The session is determined by the [`SendTarget`], never by a global
    /// "current" session:
    ///
    /// - [`SendTarget::Thread`] — an existing conversation. The session is
    ///   derived from the thread (threads belong to a session), then ensured
    ///   open before the text is dispatched:
    ///   - **Open** (a live pane is bound): the text is dispatched immediately on
    ///     the normal path — the `pending_send` row is written *before* the
    ///     keystrokes, so the correlation head is in place when the
    ///     `UserPromptSubmit` hook fires, with the cancel-on-dispatch-failure
    ///     rollback below.
    ///   - **Closed** (the session exists in the store but no live pane): it is
    ///     resumed via [`Self::open_session`] (`claude --resume <id>`), then the
    ///     normal path runs.
    /// - [`SendTarget::NewSession`] — a composer-first message. A fresh session
    ///   is spawned with the text deferred as its `first_prompt`. The
    ///   `pending_send` row cannot be written yet (it references a session id
    ///   that does not exist), so it is held on the spawn and written when the
    ///   first `UserPromptSubmit` binds the spawn. A synthetic, not-yet-persisted
    ///   [`PendingSend`] is returned so the REST surface has a response, carrying
    ///   the still-unknown target thread as `0` (the real id is assigned at bind
    ///   time on the new session's `main`).
    ///
    /// A branch send (the `branch_from` arm of [`SendTarget::Thread`]) requires
    /// an existing session — there must be a message to branch from — which the
    /// thread target inherently provides.
    pub async fn enqueue_send(
        &self,
        target: SendTarget,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<PendingSend> {
        match target {
            SendTarget::Thread {
                thread_id,
                branch_from,
            } => {
                // Derive the owning session from the target thread. A stale or
                // wrong id becomes a clean `ThreadNotFound` (404) rather than an
                // opaque failure downstream.
                let thread = self
                    .store
                    .thread(thread_id)
                    .await?
                    .ok_or_else(|| Error::ThreadNotFound(thread_id.value()))?;
                let session_id = thread.session_id;
                // Ensure the session is open: resume it if it is known but closed
                // (no live pane). Once open we have a pane to dispatch to and the
                // normal pre-dispatch path applies.
                let pane = self.ensure_open(&session_id).await?;
                self.enqueue_into_open(
                    &session_id,
                    &pane,
                    thread_id,
                    text,
                    locator_quote,
                    branch_from.as_ref(),
                )
                .await
            }
            SendTarget::NewSession { workdir } => {
                // No session yet: spawn one with the text deferred as its first
                // prompt, in the user-selected `workdir` when given (validated by
                // `spawn_fresh` before any pane is created) or the default
                // per-spawn directory otherwise. The real `pending_send` row is
                // written when the first `UserPromptSubmit` binds the spawn.
                //
                // `locator_quote` is intentionally dropped here, not forwarded to
                // the spawn: a brand-new session has no earlier passage to anchor,
                // so there is nothing to locate. It is still echoed in the
                // synthetic response below as a courtesy to the caller, but the
                // deferred first prompt (and the row written at bind time) carry
                // no quote.
                self.spawn_fresh(Some(text.to_owned()), workdir).await?;
                Ok(deferred_pending_send(text, locator_quote))
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
        // The target thread was already loaded by the caller to derive the
        // session, so its existence is established here (a stale/wrong id surfaced
        // as `ThreadNotFound` before reaching this point).
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

    /// Register a session on the first `UserPromptSubmit` for its id, binding it
    /// to a fresh spawn when one is waiting.
    ///
    /// The first time Claude Code reports a `session_id`, two cases are
    /// distinguished by whether that id matches a pending spawn:
    ///
    /// - **Fresh spawn binding**: a [`PendingSpawn`] whose Delta-minted
    ///   `session_id` (pinned via `claude --session-id`) equals the hook's
    ///   `session_id` is moved `pending → bound[session_id]`. The session row is
    ///   registered (from the hook's `cwd`/`transcript_path`), and if the spawn
    ///   carried a deferred `first_prompt` (a composer-initiated New), the held
    ///   `pending_send` is written *now* — with the now-known session id —
    ///   *before* the caller's `match_pending_send` runs, so the first prompt
    ///   correlates through the normal FIFO machinery.
    /// - **External claude**: no pending spawn carries this session id, so this
    ///   is a `claude` started outside Delta. The session is registered as a
    ///   known-but-closed data session (no [`OpenHandle`]) and a warning is
    ///   logged, preserving today's external-input behaviour.
    async fn register_on_first_contact(
        &self,
        hook: &UserPromptSubmitHook,
        events: &mut Vec<SessionEvent>,
    ) -> Result<Session> {
        // Match a waiting spawn by the Delta-minted session id under the
        // registry lock, taking its deferred first prompt with it.
        let bound = {
            let mut registry = self.open_sessions.lock().await;
            match registry.take_pending_for_session(&hook.session_id) {
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
        // Resolve the `additionalContext` note *before* syncing, so the current
        // user line is not yet ingested and `latest_user_thread` still reports
        // the PREVIOUS thread the user was in — letting us detect a switch.
        let additional_context = self
            .thread_switch_context(&hook.session_id, pending.as_ref())
            .await?;

        // Ingest new transcript lines. This matches each user line to its queued
        // send and attributes it (plus the assistant lines that follow it) to
        // the right thread, marking the send matched as a side effect. Any
        // permission-resolution events the ingest produced are broadcast too.
        let (new_messages, resolved_events) = self.sync_transcript(&session).await?;
        events.extend(resolved_events);

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
        let mut events = Vec::new();
        // Route by the hook's own session id so the right session's transcript is
        // synced, even when several sessions are registered. The final transcript
        // lines often include the last tool_result, so the `Stop` sync is a key
        // place permission requests resolve.
        if let Some(session) = self.store.session(&hook.session_id).await? {
            let (_messages, resolved_events) = self.sync_transcript(&session).await?;
            events.extend(resolved_events);
        }
        events.push(SessionEvent::TurnCompleted {
            session_id: hook.session_id,
            stop_reason: hook.stop_reason,
        });
        Ok(events)
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
    /// lock), so it is safe to call concurrently with the hook handlers.
    ///
    /// Alongside the per-session message groups, returns any [`SessionEvent`]s
    /// the ingest produced (e.g. [`SessionEvent::PermissionResolved`] when a
    /// late `tool_result` is tailed in) for the caller to broadcast. Most
    /// tool_results are ingested here by the continuous tail, so this is the
    /// primary path that clears an auto-approved tool's notice. Returns empty
    /// when no session has been registered yet.
    pub async fn poll_transcript(&self) -> Result<(Vec<Vec<Message>>, Vec<SessionEvent>)> {
        let mut groups = Vec::new();
        let mut events = Vec::new();
        for session in self.store.list_sessions().await? {
            let (messages, resolved_events) = self.sync_transcript(&session).await?;
            events.extend(resolved_events);
            if !messages.is_empty() {
                groups.push(messages);
            }
        }
        Ok((groups, events))
    }

    /// Handle a `PreToolUse` hook: record the request for UI/audit and notify
    /// the browser. Delta never returns allow/deny — the TUI owns that.
    pub async fn on_pre_tool_use(
        &self,
        session_id: &delta_model::SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: &str,
    ) -> Result<Vec<SessionEvent>> {
        let request = self
            .store
            .record_permission_request(session_id, tool_name, tool_input_json, tool_use_id)
            .await?;
        Ok(vec![SessionEvent::PermissionRequested {
            session_id: session_id.clone(),
            request_id: request.id,
            tool_name: tool_name.to_owned(),
        }])
    }

    /// Browse the immediate subdirectories of `path` for the directory picker.
    ///
    /// Delegates to [`Workspace::list_dirs`], which returns the canonical path,
    /// its parent, and the immediate subdirectories (dirs only, dot-directories
    /// excluded, sorted by name). A `None` or empty `path` defaults to the user's
    /// home directory so the picker has a sensible starting point. A missing
    /// path, a non-directory, or a permission error surfaces as a clean
    /// `InvalidWorkdir`/`WorkdirPermission` rather than an internal failure.
    pub async fn browse_workdir(&self, path: Option<&str>) -> Result<DirListing> {
        let start = match path {
            Some(p) if !p.is_empty() => p.to_owned(),
            _ => home_dir()?,
        };
        self.workspace.list_dirs(&start).await
    }

    /// The recently-used working directories for the picker's "recent" list.
    ///
    /// Distinct `session.cwd` values, most-recently-used first, capped at
    /// [`RECENT_WORKDIRS_LIMIT`]. Derived from existing session rows (Delta keeps
    /// no separate history), so a directory appears here once any session has run
    /// in it.
    pub async fn recent_workdirs(&self) -> Result<Vec<RecentWorkdir>> {
        self.store.recent_workdirs(RECENT_WORKDIRS_LIMIT).await
    }

    /// Every registered session, annotated with its live state and `main` thread.
    ///
    /// Lists all sessions from the store (ordered by creation) and tags each with
    /// whether the registry currently holds a live pane for it, plus its trunk
    /// thread id. This is the browser's hydration surface: it shows every known
    /// conversation — open or closed — so the navigator can route into any of
    /// them. Returns an empty list until the first `UserPromptSubmit` registers a
    /// session (Claude Code never fires `SessionStart`).
    pub async fn list_sessions(&self) -> Result<Vec<SessionListing>> {
        let sessions = self.store.list_sessions().await?;
        let mut out = Vec::with_capacity(sessions.len());
        for session in sessions {
            let main_thread_id = self.store.main_thread_id(&session.id).await?;
            let open = self.is_session_open(&session.id).await;
            let last_activity_at = self.store.last_activity_at(&session.id).await?;
            out.push(SessionListing {
                session,
                open,
                main_thread_id,
                last_activity_at,
            });
        }
        // Most-recently-active first. The recency key is the session's last
        // activity (`MAX(message.created_at)`), falling back to its own
        // `created_at` when it has no messages yet — a brand-new, message-less
        // session sorts near the top because its `created_at` is "now". Ties
        // break deterministically on `created_at` then `id` so equal-activity
        // sessions keep a stable order across calls. ISO-8601 UTC timestamps
        // are lexicographically ordered, so a string compare is a time compare.
        out.sort_by(|a, b| {
            // Recency key: last activity, or the session's own `created_at`
            // when message-less.
            fn recency(s: &SessionListing) -> &str {
                s.last_activity_at
                    .as_deref()
                    .unwrap_or(s.session.created_at.as_str())
            }
            // Reverse `recency` and `created_at` so the most recent comes
            // first; the `id` tiebreaker stays ascending for a deterministic
            // total order.
            recency(b)
                .cmp(recency(a))
                .then_with(|| b.session.created_at.cmp(&a.session.created_at))
                .then_with(|| a.session.id.as_str().cmp(b.session.id.as_str()))
        });
        Ok(out)
    }

    /// One page of the session list, ordered most-recently-active first, with
    /// an opaque-able cursor to fetch the next page.
    ///
    /// This is the paginated form of [`Self::list_sessions`]: the store pushes
    /// the recency ordering into SQL and returns at most `limit` rows plus each
    /// row's inline `last_activity_at`, so there is no per-row activity lookup.
    /// Each row is then enriched with its live `open` state (process-runtime
    /// data the registry owns, not a SQL column) and its `main` thread id.
    ///
    /// The returned [`SessionPage::next`] cursor names the last listing's
    /// `(recency, created_at, id)` so the caller can resume strictly after it;
    /// it is `Some` only when the page came back full (more rows may follow).
    pub async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<SessionPage> {
        let rows = self.store.list_sessions_page(cursor, limit).await?;
        let full = rows.len() as u32 == limit;

        let mut listings = Vec::with_capacity(rows.len());
        for (session, last_activity_at) in rows {
            let main_thread_id = self.store.main_thread_id(&session.id).await?;
            let open = self.is_session_open(&session.id).await;
            listings.push(SessionListing {
                session,
                open,
                main_thread_id,
                last_activity_at,
            });
        }

        // The next cursor names the last row's sort key, where `recency` is the
        // listing's `last_activity_at` or its `created_at` fallback. It is only
        // meaningful when the page was full; a short/last page yields `None`.
        let next = match (full, listings.last()) {
            (true, Some(last)) => Some(SessionPageCursor {
                recency: last
                    .last_activity_at
                    .clone()
                    .unwrap_or_else(|| last.session.created_at.clone()),
                created_at: last.session.created_at.clone(),
                id: last.session.id.as_str().to_owned(),
            }),
            _ => None,
        };

        Ok(SessionPage { listings, next })
    }

    /// The thread tree for a specific session, ordered by creation.
    ///
    /// A stale or unknown session id is reported as a clean `SessionNotFound`
    /// (404) rather than yielding a silently empty list, so the browser can tell
    /// "no threads yet" apart from "no such session".
    pub async fn threads_for(&self, session_id: &SessionId) -> Result<Vec<Thread>> {
        if self.store.session(session_id).await?.is_none() {
            return Err(Error::SessionNotFound(session_id.as_str().to_owned()));
        }
        self.store.list_threads(session_id).await
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
    /// - A **human** user line (a user line carrying author-written text) that
    ///   matches a still-`pending` send is attributed to that send's thread (the
    ///   new child thread for a branch send), the send is marked matched, and
    ///   `carry_thread` advances to it. A human user line with no matching send
    ///   is external input and lands on `main`, resetting `carry_thread`.
    /// - Every other line follows `carry_thread` — the thread of the turn it
    ///   belongs to. This covers assistant/system lines AND tool-result lines,
    ///   which Claude delivers as `role: user` but which are part of the
    ///   in-flight turn, not a new human turn.
    ///
    /// Returns the newly-ingested messages and any [`SessionEvent`]s that the
    /// ingest produced. The only such event today is
    /// [`SessionEvent::PermissionResolved`]: when a `tool_result` line is
    /// ingested, the open permission request correlated by its `tool_use_id` is
    /// resolved so the browser can clear the "permission requested" notice. The
    /// caller is responsible for broadcasting these events.
    async fn sync_transcript(
        &self,
        session: &Session,
    ) -> Result<(Vec<Message>, Vec<SessionEvent>)> {
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
            return Ok((Vec::new(), Vec::new()));
        }

        // The turn in progress when this batch starts: the thread of the most
        // recent persisted user message, defaulting to `main`.
        let mut carry_thread = self
            .store
            .latest_user_thread(&session.id)
            .await?
            .unwrap_or(main_thread);

        let mut messages = Vec::with_capacity(read.messages.len());
        let mut events = Vec::new();
        for line in read.messages {
            let content_text = Message::flatten_text(&line.content);

            // Correlate any tool_result blocks on this line with an open
            // permission request keyed by `tool_use_id`. Resolving on actual
            // completion (rather than at `PreToolUse` time) is what lets an
            // auto-approved tool's notice clear immediately while a genuine TUI
            // prompt's notice persists until the human answers. A denied tool
            // yields `is_error: true` ("User rejected tool use"), so the error
            // flag infers allowed vs denied.
            for block in &line.content {
                if let delta_model::ContentBlock::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } = block
                {
                    if let Some(request_id) = self
                        .store
                        .resolve_permission_by_tool_use_id(&session.id, tool_use_id, !is_error)
                        .await?
                    {
                        events.push(SessionEvent::PermissionResolved {
                            session_id: session.id.clone(),
                            request_id,
                        });
                    }
                }
            }

            // A genuine human turn is a user line with author-written text.
            // Claude delivers tool results as `role: user` lines too, but those
            // belong to the in-flight turn, not a new human turn, so they must
            // inherit `carry_thread` rather than reset it to `main`. (Mirrors the
            // frontend's `isUserTurn`.) Treating a tool_result as a turn boundary
            // used to drop the rest of a sub-thread's turn onto `main`.
            let trimmed = content_text.as_deref().unwrap_or("").trim();
            let is_human_turn =
                matches!(line.role, delta_model::Role::User) && !trimmed.is_empty();

            let (thread_id, semantic_parent_uuid) = if is_human_turn {
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
        Ok((messages, events))
    }

    /// Compute the `additionalContext` note to inject for this prompt.
    ///
    /// Branching in Delta is view-only: the model only ever sees the single
    /// linear `parentUuid` chain, never Delta's thread tree. So when the user
    /// moves to a different thread and keeps talking, the model has no signal
    /// that the topic changed and may misread an utterance like "is that what
    /// you mean?" as referring to the (unrelated) message immediately above.
    /// This produces a short natural-language note that gives the model that
    /// missing signal.
    ///
    /// Must be called *before* `sync_transcript`, so the current user line is
    /// not yet ingested and [`SessionStore::latest_user_thread`] still reports
    /// the PREVIOUS thread the user was in. Four cases:
    ///
    /// 1. No queued send matched this prompt → external input → inject nothing.
    /// 2. The send carries a locator quote → first entry into a branch → keep
    ///    the locator-quote frame and bind it to the target thread.
    /// 3. No locator and the previous thread is KNOWN and differs from the
    ///    target → a thread switch / re-visit → inject a re-focus note (with the
    ///    target thread's root quote, unless it is `main`).
    /// 4. No locator and either the target thread is unchanged or the previous
    ///    thread is unknown (first turn / first prompt after a resume) → not a
    ///    switch → inject nothing.
    async fn thread_switch_context(
        &self,
        session_id: &SessionId,
        pending: Option<&PendingSend>,
    ) -> Result<Option<String>> {
        // Case 1: external input — no queued send to attribute → inject nothing.
        let Some(pending) = pending else {
            return Ok(None);
        };
        let cur = pending.thread_id;

        // Case 2: first entry into a branch — the user selected a passage to
        // anchor this message. Keep the locator-quote frame and tell the model
        // that this quote roots the thread it is now in.
        if let Some(quote) = pending.locator_quote.as_deref() {
            if let Some(frame) = frame_locator_context(quote) {
                return Ok(Some(frame_branch_entry_context(&frame, cur)));
            }
        }

        // Cases 3 & 4 hinge on whether the active thread changed. `prev` is the
        // thread of the latest already-persisted user line (this prompt's line
        // is not synced yet), i.e. the thread the user was in before this send.
        //
        // Only a KNOWN switch warrants a re-focus note. `prev == None` means the
        // previous thread is unknown — there is no persisted user line yet. That
        // happens on the very first turn and, crucially, on the first prompt
        // after a session resume (the prior turn's user line is not visible to
        // `latest_user_thread` at the resume boundary, since this runs before
        // `sync_transcript`). Asserting a switch there is false: injecting a
        // "switched to thread:N" note misleads the model into treating an
        // ordinary continuation as a re-visit to an earlier discussion. So a
        // switch is asserted only when `prev` is known and differs from `cur`
        // (Case 4 — same/unknown thread — falls through to no injection).
        let prev = self.store.latest_user_thread(session_id).await?;
        let Some(prev) = prev.filter(|p| *p != cur) else {
            return Ok(None);
        };

        // Case 3: thread switch / re-visit. Cite the target thread's root quote
        // so the re-focus survives even if the original binding scrolled out of
        // context. `main` has no root quote, so it is cited by name only.
        let root_quote = match self.store.thread(cur).await? {
            Some(thread) => thread.root_message_uuid.is_some().then_some(thread.title),
            None => None,
        };
        Ok(Some(frame_thread_switch_context(
            prev,
            cur,
            root_quote.as_deref(),
        )))
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
/// `Pending`, and both the session id and target thread are left empty/`0`
/// because neither exists yet (the real row is written on the new session's
/// `main` thread at bind time).
fn deferred_pending_send(text: &str, locator_quote: Option<&str>) -> PendingSend {
    PendingSend {
        id: 0,
        session_id: SessionId::from(""),
        thread_id: ThreadId(0),
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
        "The user is replying to this passage they selected from earlier in the conversation:\n{}",
        delimit_quote(quote)
    ))
}

/// Delimit a passage so a provenance frame and its quoted text stay
/// distinguishable. Centralised so every frame quotes passages the same way.
fn delimit_quote(quote: &str) -> String {
    format!("\"{}\"", quote.trim())
}

/// Extend a locator-quote frame with the thread the selected passage roots.
///
/// On the first message into a fresh branch the user has selected a passage to
/// anchor it; [`frame_locator_context`] already frames that passage. This adds a
/// note binding that passage to the thread the conversation is now in, as a
/// stable `thread:N` handle, so a later return to the same thread can re-cite it
/// by id. The id is just a handle — the quote carries the meaning.
///
/// Isolated so the exact wording is easy to tune; affects only the model-facing
/// `additionalContext`, never an on-screen message or stored field.
fn frame_branch_entry_context(locator_frame: &str, thread: ThreadId) -> String {
    format!(
        "{locator_frame}\nThat passage starts a separate thread (thread:{}); the user is now talking in that thread.",
        thread.value()
    )
}

/// Frame a switch back to an existing thread for injection as
/// `additionalContext`.
///
/// Delta's threads are invisible to the model, which sees only the linear
/// transcript. When the user moves to a different existing thread and continues
/// without selecting a new passage, this note tells the model the topic changed:
/// the continuation belongs to the named earlier thread, NOT the message
/// immediately above. The target thread's root quote (`root_quote`) is re-cited
/// so the re-focus holds even if the original binding scrolled out of context.
///
/// `prev` is the thread the user was just in; naming both endpoints makes the
/// move explicit. A switch is only asserted when the previous thread is known
/// and differs from the current one, so `prev` is always a concrete thread
/// here. The trunk thread (`main`) has no root quote, so `root_quote` is `None`
/// there and it is referred to by name only.
///
/// Isolated so the exact wording is easy to tune; affects only the model-facing
/// `additionalContext`, never an on-screen message or stored field.
fn frame_thread_switch_context(prev: ThreadId, cur: ThreadId, root_quote: Option<&str>) -> String {
    let mut note = format!(
        "The user has switched from thread:{} to thread:{}",
        prev.value(),
        cur.value()
    );
    match root_quote {
        Some(quote) if !quote.trim().is_empty() => note.push_str(&format!(
            ", the thread rooted at this passage:\n{}",
            delimit_quote(quote)
        )),
        // `main` (or a thread with no root passage): refer to it by name only.
        _ => note.push_str(" (the main thread)"),
    }
    note.push_str(
        ".\nThey are continuing that earlier discussion, not replying to the message immediately above.",
    );
    note
}

/// The user's home directory, the default starting point for directory browsing.
///
/// Read from `HOME`. An absent or empty `HOME` leaves the picker with no
/// sensible default, so it is reported as an `InvalidWorkdir` rather than
/// browsing some arbitrary fallback.
fn home_dir() -> Result<String> {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => Ok(home),
        _ => Err(Error::InvalidWorkdir(
            "HOME is not set; specify a path to browse".to_owned(),
        )),
    }
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
