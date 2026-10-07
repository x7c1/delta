//! Shared application state.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, watch};

use delta_bootstrap::{AppInteractor, Config};
use delta_usecase::{
    AsyncEventReceiver, AsyncEventSink, CommsLogSink, NewerRelease, NotOffered, ReleaseCheck,
    ReleaseUpdate, SessionEvent, SessionLifecycle,
};

use crate::comms_log::{CommsLogHub, CommsSubscription};
use crate::serve::ServerStopped;
use crate::storage_inventory::StorageInventory;

/// Capacity of the per-process event broadcast channel.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// How often the background tail polls the transcript for new lines.
///
/// Claude Code often flushes the final assistant line after the `Stop` hook
/// fires, so the hook sync misses it. A sub-second poll picks it up so the reply
/// renders without waiting for the next hook.
const TRANSCRIPT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long the three registry sweeps on the same tick (resume dispatch, spawn
/// reap, echo-deadline sweep) wait on any one session actor.
///
/// Deliberately far longer than [`TRANSCRIPT_POLL_INTERVAL`], which bounds the
/// transcript poll, because the two fan-outs lose different things to a missed
/// deadline. The poll's actors announce themselves on the async event seam, so
/// skipping one costs only that tick's return value. These three have no such
/// seam: their reply *is* the delivery of their events, and dropping it strands
/// a `SendDispatched` (the browser keeps showing a just-typed send as queued) or
/// a `SpawnFailed` (the optimistic pending chip never clears) until the next
/// reload.
///
/// So the bound must sit above a *legitimately* slow actor, not at the poll
/// interval. Each of these sweeps types into a pane, and that typing waits on
/// purpose: `send_line` holds 250ms between the text and its submit `Enter`
/// (Claude's TUI would otherwise absorb the `Enter` into the paste burst), and
/// the echo sweep may send a settling `Escape` first — roughly 400ms of built-in
/// delay plus four `tmux send-keys` round trips before anything has gone wrong.
/// This is the backstop for an actor that has wedged, so that one cannot stall
/// the loop forever; it is not a scheduling budget.
const SWEEP_TICK_BOUND: Duration = Duration::from_secs(2);

/// How long after the server starts the first release check runs.
///
/// Off the startup path on purpose: an offline machine must start exactly as
/// fast as it would without the check, so the check waits until the server is
/// already serving.
///
/// The browser footer re-asks 30 s after its first answer to catch this first
/// check (`LATEST_RELEASE_FIRST_RECHECK_MS` in the frontend's api-client), so
/// this delay plus the feed's request timeout must stay below that.
const RELEASE_CHECK_FIRST_DELAY: Duration = Duration::from_secs(5);

/// How often the release check runs after the first one. GitHub allows 60
/// unauthenticated requests an hour; this is far below that.
const RELEASE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// State shared across all request handlers.
///
/// Cheap to clone: the Interactor and broadcast sender are reference-counted.
#[derive(Clone)]
pub struct AppState {
    interactor: Arc<AppInteractor>,
    events: broadcast::Sender<SessionEvent>,
    /// The receiving half of the interactor's async event seam, drained once by
    /// [`Self::spawn_async_event_drain`] into [`Self::events`]. Held behind a
    /// `Mutex<Option<..>>` because a receiver cannot be cloned (this state is
    /// `Clone`) and is consumed by exactly one drain task; `take()` hands it out
    /// once and later calls (or clones) get `None`.
    async_events: Arc<std::sync::Mutex<Option<AsyncEventReceiver>>>,
    /// Delta's dedicated tmux socket, so the PTY bridge attaches on the same
    /// server the sessions live on (`tmux -L <socket> attach-session`).
    tmux_socket: Arc<str>,
    /// The environment variables the PTY bridge sets on its `tmux attach`
    /// client: the configuration's [`Config::child_env`]. Empty unless
    /// [`Self::with_child_env`] sets it.
    child_env: Arc<[(String, String)]>,
    /// The per-run bearer token every browser request must present, enforced by
    /// [`crate::auth_guard`]. Minted (or handed in) once for the server's
    /// lifetime — see `config::config_from_env` — and never rotated. Held as an
    /// `Arc<str>` mirroring [`Self::tmux_socket`], so cloning the state is cheap.
    auth_token: Arc<str>,
    /// The hook secret every `/hooks/*` request must carry as an `?hs=` query
    /// parameter, enforced by [`crate::hook_auth_guard`]. Fixed for the
    /// server's lifetime and, unlike [`Self::auth_token`], kept across restarts
    /// in the hook state file — see `config::adopt_persisted_hook_secret` — and
    /// rendered into the session settings so genuine Claude Code callbacks
    /// present it. Held as an `Arc<str>` mirroring [`Self::auth_token`], so
    /// cloning the state is cheap.
    hook_secret: Arc<str>,
    /// Where this server keeps its files, for `GET /api/storage`. Paths only,
    /// fixed at startup; the sizes are read on each request.
    storage: Arc<StorageInventory>,
    /// The per-session comms log the `/comms` stream serves.
    ///
    /// The same instance the adapters record into (it is handed to the
    /// composition root as their [`CommsLogSink`]), so a frame an adapter emits
    /// and a frame the browser reads are two views of one buffer.
    comms_log: Arc<CommsLogHub>,
    /// Why the server is to stop, once something has asked it to: empty while
    /// it serves. [`crate::serve::serve`] shuts down gracefully when this is
    /// set, and the browser event stream ends its sockets.
    stop: Arc<watch::Sender<Option<ServerStopped>>>,
    /// Set by the first `POST /api/storage/erase` and never cleared once that
    /// erase succeeds — the server stops right after — so a second erase is
    /// refused rather than run twice.
    erasing: Arc<AtomicBool>,
    /// The background check for a newer published release, holding its last
    /// verdict for `GET /api/latest-release`. Turned off unless
    /// [`Self::with_release_check`] sets one.
    release_check: Arc<ReleaseCheck>,
    /// The download of the newer release [`Self::release_check`] found, for
    /// `POST /api/latest-release/download`. Not offered unless
    /// [`Self::with_release_update`] sets one that is.
    release_update: Arc<ReleaseUpdate>,
}

impl AppState {
    /// Build the shared state from configuration, wiring the Interactor.
    ///
    /// Async because the composition root's boot-time send reconcile (the
    /// sweep returning restart-orphaned `dispatched` rows to `queued`) runs
    /// against the freshly-opened store before the state is handed out, and
    /// so does the re-adoption of the sessions whose panes survived the
    /// restart ([`Self::readopt_surviving_sessions`]).
    pub async fn build(config: &Config) -> anyhow::Result<Self> {
        // The comms log is created here, before the composition root wires the
        // adapters, because it is the one gateway BOTH sides need: the adapters
        // record into it and the `/comms` route reads it. Handing the same
        // instance to both is what makes the pane show live frames.
        let comms_log = Arc::new(CommsLogHub::new());
        let interactor =
            delta_bootstrap::build(config, Arc::clone(&comms_log) as Arc<dyn CommsLogSink>).await?;
        let state = Self::from_interactor(
            interactor,
            &config.tmux_socket,
            &config.auth_token,
            &config.hook_secret,
            StorageInventory::from_config(config),
        )
        .with_comms_log(comms_log)
        .with_child_env(config.child_env.clone())
        .with_release_check(delta_bootstrap::release_check(
            config,
            crate::version::VERSION,
        ))
        .with_release_update(delta_bootstrap::release_update(config));
        state
            .readopt_surviving_sessions(config.hook_endpoint_changed)
            .await;
        Ok(state)
    }

    /// Re-adopt the Claude Code sessions whose tmux panes survived the restart,
    /// before the server accepts its first request.
    ///
    /// Run here rather than in the composition root because it starts session
    /// actors, and the interactor's async event seam must be wired (by
    /// [`Self::from_interactor`]) before any actor exists. Running it before the
    /// listener is served is what keeps a send from racing it; the use case's
    /// resume backstop covers one that does anyway.
    ///
    /// A failure here is logged, not returned: the sessions it could not look
    /// at stay closed with their record kept, and that backstop still adopts
    /// each one before a send could resume it into a second process. Refusing
    /// to start over it would cost the user every session instead of a few.
    async fn readopt_surviving_sessions(&self, hook_endpoint_changed: bool) {
        match self
            .interactor
            .readopt_surviving_sessions(hook_endpoint_changed)
            .await
        {
            Ok(summary) => tracing::info!(
                adopted = summary.adopted,
                hooks_unreachable = summary.hooks_unreachable,
                gone = summary.gone,
                unprobed = summary.unprobed,
                failed = summary.failed,
                "re-adopted the sessions that survived the restart"
            ),
            Err(err) => tracing::error!(
                error = %err,
                "could not list the sessions that may have survived the restart; \
                 they stay closed until a send re-adopts or resumes each one"
            ),
        }
    }

    /// Build the shared state from an already-wired Interactor.
    ///
    /// The Interactor's gateways are type-erased (see [`AppInteractor`]), so
    /// integration tests can inject fakes (an in-memory store, a temp-file
    /// transcript, a no-op tmux driver) and still produce this exact
    /// [`AppState`] type — no generics leak into the transport layer. The spawn
    /// configuration (base workdir, hook settings) lives inside the Interactor;
    /// `storage` names the paths `GET /api/storage` reports.
    pub fn from_interactor(
        interactor: AppInteractor,
        tmux_socket: &str,
        auth_token: &str,
        hook_secret: &str,
        storage: StorageInventory,
    ) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        // Wire the interactor's async event seam here — before the interactor is
        // shared (`Arc`-wrapped) and any session actor spawns, which
        // `with_event_sink` requires. The interactor side pushes on the sink;
        // this state keeps the receiver for `spawn_async_event_drain` to forward
        // into the broadcast above. Every `AppState` — production and test — gets
        // the seam wired uniformly this way.
        let (sink, async_rx) = AsyncEventSink::channel();
        let interactor = interactor.with_event_sink(sink);
        Self {
            interactor: Arc::new(interactor),
            events,
            async_events: Arc::new(std::sync::Mutex::new(Some(async_rx))),
            tmux_socket: Arc::from(tmux_socket),
            child_env: Arc::from([]),
            auth_token: Arc::from(auth_token),
            hook_secret: Arc::from(hook_secret),
            storage: Arc::new(storage),
            // An unwired log: `/comms` then serves an always-idle stream, which
            // is exactly right for a state whose interactor records nowhere.
            // `build` (and any test that wants live frames) replaces it via
            // `with_comms_log`.
            comms_log: Arc::new(CommsLogHub::new()),
            stop: Arc::new(watch::Sender::new(None)),
            erasing: Arc::new(AtomicBool::new(false)),
            release_check: Arc::new(ReleaseCheck::disabled()),
            release_update: Arc::new(ReleaseUpdate::not_offered(NotOffered::CliLauncher)),
        }
    }

    /// Serve `/comms` from `hub` — the same instance the wired adapters record
    /// into.
    ///
    /// Separate from [`Self::from_interactor`] because the interactor is built
    /// first (the composition root needs the sink to wire the adapters) and every
    /// existing test builds its state without one; without this, a state whose
    /// adapters record into a hub would serve a *different*, permanently empty
    /// hub, and the pane would look idle during a live turn.
    pub fn with_comms_log(mut self, hub: Arc<CommsLogHub>) -> Self {
        self.comms_log = hub;
        self
    }

    /// Set `env` on the commands the transport layer starts itself (the PTY
    /// bridge's `tmux attach`), as the gateways the interactor was wired with
    /// set it on theirs.
    ///
    /// Separate from [`Self::from_interactor`] for the same reason as
    /// [`Self::with_comms_log`]: every test that builds its state from an
    /// interactor runs with the inherited environment.
    pub fn with_child_env(mut self, env: Vec<(String, String)>) -> Self {
        self.child_env = env.into();
        self
    }

    /// Check for newer releases with `check`.
    ///
    /// Separate from [`Self::from_interactor`] for the same reason as
    /// [`Self::with_comms_log`]: a state built from an interactor in a test
    /// never reaches GitHub.
    pub fn with_release_check(mut self, check: ReleaseCheck) -> Self {
        self.release_check = Arc::new(check);
        self
    }

    /// Download newer releases with `update`.
    ///
    /// Separate from [`Self::from_interactor`] for the same reason as
    /// [`Self::with_release_check`]: a state built from an interactor in a
    /// test never downloads anything.
    pub fn with_release_update(mut self, update: ReleaseUpdate) -> Self {
        self.release_update = Arc::new(update);
        self
    }

    /// The newer release the last successful release check found, if any.
    pub fn newer_release(&self) -> Option<NewerRelease> {
        self.release_check.newer()
    }

    /// The download of the newer release.
    pub(crate) fn release_update(&self) -> &ReleaseUpdate {
        &self.release_update
    }

    /// Spawn the background release check: once [`RELEASE_CHECK_FIRST_DELAY`]
    /// after startup, then every [`RELEASE_CHECK_INTERVAL`].
    ///
    /// `None` when the check is turned off, so nothing is spawned and nothing
    /// is ever asked. A failed check is logged by the check itself and keeps
    /// its previous verdict; the loop carries on.
    pub fn spawn_release_check(&self) -> Option<tokio::task::JoinHandle<()>> {
        if !self.release_check.is_enabled() {
            return None;
        }
        let check = Arc::clone(&self.release_check);
        Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval_at(
                tokio::time::Instant::now() + RELEASE_CHECK_FIRST_DELAY,
                RELEASE_CHECK_INTERVAL,
            );
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                // The error is already logged as one `warn` by the check.
                let _ = check.check().await;
            }
        }))
    }

    /// Watch one session's comms log: buffered frames, then the live tail.
    ///
    /// What the `/comms` route pumps into its socket, and — since it is the whole
    /// contract minus the socket bytes — what an integration test asserts on
    /// without standing up a WebSocket client.
    pub fn watch_comms_log(&self, session_id: &str) -> CommsSubscription {
        self.comms_log.subscribe(session_id)
    }

    /// Delta's dedicated tmux socket name (`tmux -L <socket>`).
    pub fn tmux_socket(&self) -> &str {
        &self.tmux_socket
    }

    /// The environment variables set on the commands the server starts; see
    /// [`Self::with_child_env`].
    pub fn child_env(&self) -> &[(String, String)] {
        &self.child_env
    }

    /// The per-run bearer token every browser request must present. Read by
    /// [`crate::auth_guard`] to authorize a request, and by the integration
    /// tests to attach a valid token to the requests they drive through the
    /// router.
    pub fn token(&self) -> &str {
        &self.auth_token
    }

    /// The hook secret every `/hooks/*` request must present as an `?hs=`
    /// query parameter. Read by [`crate::hook_auth_guard`] to authorize a hook
    /// callback, and by the integration tests to attach a valid secret to the
    /// hook requests they drive through the router.
    pub fn hook_secret(&self) -> &str {
        &self.hook_secret
    }

    /// Where this server keeps its files, for `GET /api/storage`.
    pub fn storage(&self) -> &StorageInventory {
        &self.storage
    }

    /// Claim the one erase this server runs, returning `false` when one is
    /// already running (or has run and the server is stopping).
    pub(crate) fn begin_erase(&self) -> bool {
        self.erasing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Give the claim back after an erase that failed, so the user can try
    /// again.
    pub(crate) fn abandon_erase(&self) {
        self.erasing.store(false, Ordering::SeqCst);
    }

    /// Ask the server to stop serving, for `reason`.
    ///
    /// Graceful: a response in flight — the erase's own — is still sent.
    pub(crate) fn stop(&self, reason: ServerStopped) {
        self.stop.send_replace(Some(reason));
    }

    /// Resolves once [`Self::stop`] has been called.
    pub(crate) fn stopping(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut stop = self.stop.subscribe();
        async move {
            // An error means every sender is gone, which only happens once
            // the server is torn down: stopping either way.
            let _ = stop.wait_for(Option::is_some).await;
        }
    }

    /// Take why the server was asked to stop, if it was.
    pub(crate) fn take_stop_reason(&self) -> Option<ServerStopped> {
        self.stop.send_replace(None)
    }

    /// The wired Interactor.
    pub fn interactor(&self) -> &AppInteractor {
        &self.interactor
    }

    /// The tmux pane the PTY bridge may attach to for a session, recording the
    /// attach as it resolves it.
    ///
    /// Delegates to the use case, which defines what resolves (`None` when there
    /// is nothing to attach to) and what `bound` means. Every `Some` must be
    /// paired with [`Self::detach_pane`]; the bridge holds a drop guard for
    /// that.
    pub async fn attach_pane(
        &self,
        id: &delta_usecase::SessionId,
    ) -> Option<delta_usecase::AttachablePane> {
        self.interactor.attach_pane(id).await
    }

    /// Give back an attach recorded by [`Self::attach_pane`], when its bridge
    /// ends.
    ///
    /// `Instant::now()` is the live clock here, as it is for the background
    /// ticks below: the use case takes the instant rather than reading one so
    /// its tests can drive the deadline a detach restarts.
    pub async fn detach_pane(&self, id: &delta_usecase::SessionId) {
        self.interactor.detach_pane(id, Instant::now()).await;
    }

    /// Wipe the residual input of a session's open pane, for the PTY bridge.
    ///
    /// Delegates to the use case. A no-op when the session is not open (there is
    /// no live pane to clear).
    pub async fn clear_session_input(
        &self,
        id: &delta_usecase::SessionId,
    ) -> delta_usecase::Result<()> {
        self.interactor.clear_session_input(id).await
    }

    /// Ensure a Claude Code session is up, spawning one lazily if absent.
    ///
    /// Delegates to the use case, which mints a fresh tmux session in its own
    /// working directory with the rendered hook settings. Idempotent: an
    /// existing open session keeps the server reporting `Ready`.
    pub async fn ensure_session(&self) -> delta_usecase::Result<SessionLifecycle> {
        self.interactor.ensure_session().await
    }

    /// Subscribe to the event stream (one receiver per browser connection).
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.events.subscribe()
    }

    /// Broadcast a batch of events to all subscribers.
    ///
    /// A send error only means there are currently no subscribers, which is not
    /// a failure for the caller.
    pub fn broadcast(&self, events: impl IntoIterator<Item = SessionEvent>) {
        for event in events {
            let _ = self.events.send(event);
        }
    }

    /// Drain the interactor's async event seam into the broadcast.
    ///
    /// The synchronous return path — hook handlers and ticks handing their
    /// `Vec<SessionEvent>` back for the caller to broadcast — is untouched. This
    /// is its asynchronous complement: a producer that emits *after* its driving
    /// call returned pushes onto the interactor's [`AsyncEventSink`], and this
    /// background task pulls each event off the matching receiver and forwards
    /// it to the same broadcast (via the raw sender clone, since `&self` is not
    /// available inside the task). The loop ends when the last sink is dropped
    /// (the interactor is gone), i.e. at shutdown.
    ///
    /// Returns `None` if the receiver was already taken (this must be called at
    /// most once per state); production calls it once at boot alongside
    /// [`Self::spawn_transcript_tail`].
    pub fn spawn_async_event_drain(&self) -> Option<tokio::task::JoinHandle<()>> {
        let mut rx = self
            .async_events
            .lock()
            .expect("async event mutex poisoned")
            .take()?;
        let events = self.events.clone();
        Some(tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let _ = events.send(event);
            }
        }))
    }

    /// Spawn the continuous transcript tail.
    ///
    /// Every [`TRANSCRIPT_POLL_INTERVAL`], poll every registered session's
    /// transcript for newly-written lines. This catches the assistant reply that
    /// Claude Code flushes to the JSONL *after* the `Stop` hook fires, which the
    /// hook sync misses.
    ///
    /// The poll's *announcements* do not come back through this loop: each
    /// session's actor emits its own transcript-updated event (and the ingest's
    /// other events) onto the interactor's async event seam the moment its rows
    /// land, and [`Self::spawn_async_event_drain`] forwards them to the same
    /// broadcast. That is deliberate — announcing here, after the fan-out had
    /// collected *every* session's reply, meant one slow actor delayed every
    /// other session's refetch signal. So the poll's return value is discarded
    /// here: re-broadcasting it would only make each browser refetch twice.
    ///
    /// Every fan-out is bounded, so an actor that stops answering cannot stall
    /// the loop; the use case logs the session and stage it dropped. The poll
    /// waits [`TRANSCRIPT_POLL_INTERVAL`] and the three sweeps below wait
    /// [`SWEEP_TICK_BOUND`] — they type into panes, and a skipped reply loses
    /// their events outright rather than just a return value (see that
    /// constant). A tick that still overruns the interval is logged with its
    /// per-stage durations.
    ///
    /// The same tick also runs three registry sweeps that must execute outside
    /// any hook handler:
    ///
    /// - **Resume dispatch**: types the held first prompt of every resume that
    ///   `SessionStart(source=resume)` marked ready and that has since settled.
    ///   The readiness hook only *marks* the resume ready — it cannot type the
    ///   prompt itself, because that hook blocks `claude` until it returns and a
    ///   keystroke sent then is lost to a still-blocked TUI. Dispatching here, a
    ///   beat after the hook returned, lands the keystroke once `claude` is
    ///   input-ready. A settled resume with no held prompt flushes the session's
    ///   oldest `queued` send instead, broadcasting the resulting
    ///   [`SessionEvent::SendDispatched`].
    /// - **Liveness reap**: reaps any fresh spawn that never bound, and any
    ///   resumed session that never became ready, before its deadline —
    ///   broadcasting the resulting [`SessionEvent::SpawnFailed`]s, so a launch
    ///   that crashed/hung (a fresh spawn before its first hook, or a
    ///   `claude --resume` that never reached `SessionStart(resume)`) can no
    ///   longer stall the UI on "pending" forever (the `SessionEnd` hook catches
    ///   the exited case immediately; this catches the hang-forever case). The
    ///   same pass also probes every *open, pane-backed* session's pane and
    ///   closes the session when that pane is gone, broadcasting
    ///   `session_closed` — an agent that exited, or was killed or crashed
    ///   without a `SessionEnd` hook, would otherwise leave a session reading as
    ///   open forever, failing every send typed into its dead pane.
    /// - **Echo watchdog**: releases any dispatched send whose keystrokes were
    ///   swallowed with no trace at all — no echo, no turn boundary, nothing —
    ///   retrying it once and then parking it, which holds it in the queue for
    ///   the user to send or cancel, so a TUI dialog eating a paste can no
    ///   longer leave the queue stuck on a permanent "in progress".
    ///
    /// All three sweeps share this loop rather than owning their own tasks —
    /// all are cheap periodic passes over the same registry.
    ///
    /// The task clones the `Arc`-shared interactor and the broadcast sender, so
    /// it stays alive independently of any request. A poll or reap error is
    /// logged and the loop continues — a transient failure must never kill the
    /// tail or the watchdog.
    pub fn spawn_transcript_tail(&self) -> tokio::task::JoinHandle<()> {
        let interactor = Arc::clone(&self.interactor);
        let events = self.events.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(TRANSCRIPT_POLL_INTERVAL);
            loop {
                ticker.tick().await;
                // One clock read drives every stage's injected `now` *and* the
                // tick's own timing, so the stage durations below partition the
                // tick exactly.
                let now = Instant::now();
                // Resume dispatch: type the held first prompt of every resume that
                // `SessionStart(source=resume)` marked ready and that has since
                // settled. This runs outside the (blocking) SessionStart hook
                // handler, so by now `claude` has returned from the hook and is
                // input-ready — the keystroke that would have been lost if typed
                // inside the handler submits here. A settled resume with no held
                // prompt flushes its session's oldest `queued` send instead
                // (queued dispatch is deferred while the resume window is open);
                // broadcast the resulting `SendDispatched` events so the browser
                // sees each queued→dispatched transition. `Instant::now()` is the
                // live clock; tests drive `dispatch_ready_resumes` directly with
                // an injected `now`.
                match interactor
                    .dispatch_ready_resumes(now, SWEEP_TICK_BOUND)
                    .await
                {
                    Ok(dispatched_events) => {
                        for event in dispatched_events {
                            let _ = events.send(event);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "resume dispatch failed");
                    }
                }
                let resumes_done = Instant::now();
                // Liveness reap: reap fresh spawns that never bound and
                // resumes that never became ready before their deadlines, and
                // close every open session whose tmux pane has gone (its agent
                // exited, died or was killed, so there is nothing left to type
                // into). `Instant::now()` is the live clock here; tests drive
                // `reap_stale_spawns` directly with an injected `now`.
                match interactor.reap_stale_spawns(now, SWEEP_TICK_BOUND).await {
                    Ok(failed_events) => {
                        for event in failed_events {
                            let _ = events.send(event);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "liveness reap failed");
                    }
                }
                let reap_done = Instant::now();
                // Echo watchdog: release any send whose keystrokes vanished
                // without a trace — no echo, no turn boundary, no signal of any
                // kind — before its deadline, retrying it once and parking it
                // (held in the queue for the user to send or cancel) if that
                // retry is swallowed too. This is the one recovery that cannot
                // be event-driven, since the failure it covers produces no
                // event to react to; the ticks are what make the silence
                // observable. `Instant::now()` is the live clock here; tests
                // drive `sweep_echo_deadlines` directly with an injected `now`.
                match interactor.sweep_echo_deadlines(now, SWEEP_TICK_BOUND).await {
                    Ok(dispatched_events) => {
                        for event in dispatched_events {
                            let _ = events.send(event);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "echo deadline sweep failed");
                    }
                }
                let echo_sweep_done = Instant::now();
                // Transcript poll. Nothing is broadcast from here: every session's
                // actor already announced its own ingest on the async event seam,
                // at the moment its rows landed, so the returned batch is dropped
                // (see this function's doc comment).
                if let Err(err) = interactor.poll_transcript(TRANSCRIPT_POLL_INTERVAL).await {
                    tracing::warn!(error = %err, "transcript tail poll failed");
                }
                // A tick that outruns its own interval starves every stage behind
                // it, so name what it spent the time on. Each fan-out is bounded,
                // so this reports a *slow* tick rather than a stuck one — the
                // use case logs the individual actors it gave up waiting for.
                let poll_done = Instant::now();
                let elapsed = poll_done - now;
                if elapsed > TRANSCRIPT_POLL_INTERVAL {
                    tracing::warn!(
                        elapsed_ms = elapsed.as_millis() as u64,
                        interval_ms = TRANSCRIPT_POLL_INTERVAL.as_millis() as u64,
                        resume_dispatch_ms = (resumes_done - now).as_millis() as u64,
                        reap_ms = (reap_done - resumes_done).as_millis() as u64,
                        echo_sweep_ms = (echo_sweep_done - reap_done).as_millis() as u64,
                        transcript_poll_ms = (poll_done - echo_sweep_done).as_millis() as u64,
                        "transcript tail tick overran its poll interval"
                    );
                }
            }
        })
    }
}
