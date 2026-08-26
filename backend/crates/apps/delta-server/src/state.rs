//! Shared application state.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;

use delta_bootstrap::{AppInteractor, Config};
use delta_usecase::{
    AsyncEventReceiver, AsyncEventSink, CommsLogSink, SessionEvent, SessionLifecycle,
};

use crate::comms_log::{CommsLogHub, CommsSubscription};

/// Capacity of the per-process event broadcast channel.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// How often the background tail polls the transcript for new lines.
///
/// Claude Code often flushes the final assistant line after the `Stop` hook
/// fires, so the hook sync misses it. A sub-second poll picks it up so the reply
/// renders without waiting for the next hook.
const TRANSCRIPT_POLL_INTERVAL: Duration = Duration::from_millis(500);

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
    /// The per-session comms log the `/comms` stream serves.
    ///
    /// The same instance the adapters record into (it is handed to the
    /// composition root as their [`CommsLogSink`]), so a frame an adapter emits
    /// and a frame the browser reads are two views of one buffer.
    comms_log: Arc<CommsLogHub>,
}

impl AppState {
    /// Build the shared state from configuration, wiring the Interactor.
    ///
    /// Async because the composition root's boot-time send reconcile (the
    /// sweep returning restart-orphaned `dispatched` rows to `queued`) runs
    /// against the freshly-opened store before the state is handed out.
    pub async fn build(config: &Config) -> anyhow::Result<Self> {
        // The comms log is created here, before the composition root wires the
        // adapters, because it is the one gateway BOTH sides need: the adapters
        // record into it and the `/comms` route reads it. Handing the same
        // instance to both is what makes the pane show live frames.
        let comms_log = Arc::new(CommsLogHub::new());
        let interactor =
            delta_bootstrap::build(config, Arc::clone(&comms_log) as Arc<dyn CommsLogSink>).await?;
        Ok(Self::from_interactor(interactor, &config.tmux_socket).with_comms_log(comms_log))
    }

    /// Build the shared state from an already-wired Interactor.
    ///
    /// The Interactor's gateways are type-erased (see [`AppInteractor`]), so
    /// integration tests can inject fakes (an in-memory store, a temp-file
    /// transcript, a no-op tmux driver) and still produce this exact
    /// [`AppState`] type — no generics leak into the transport layer. The spawn
    /// configuration (base workdir, hook settings) lives inside the Interactor.
    pub fn from_interactor(interactor: AppInteractor, tmux_socket: &str) -> Self {
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
            // An unwired log: `/comms` then serves an always-idle stream, which
            // is exactly right for a state whose interactor records nowhere.
            // `build` (and any test that wants live frames) replaces it via
            // `with_comms_log`.
            comms_log: Arc::new(CommsLogHub::new()),
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

    /// The wired Interactor.
    pub fn interactor(&self) -> &AppInteractor {
        &self.interactor
    }

    /// The tmux pane driving a specific open session, for the PTY bridge.
    ///
    /// Returns `None` when that session is not open, so the bridge can refuse the
    /// attach rather than bind to a non-existent pane.
    pub async fn pane_for_session(&self, id: &delta_usecase::SessionId) -> Option<String> {
        self.interactor.pane_for_session(id).await
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
    /// transcript for newly-written lines. For each session that ingested new
    /// lines, broadcast a [`SessionEvent::TranscriptUpdated`] carrying the
    /// distinct threads they landed on so browsers refetch them. This catches the
    /// assistant reply that Claude Code flushes to the JSONL *after* the `Stop`
    /// hook fires, which the hook sync misses.
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
    /// - **Launch watchdog**: reaps any fresh spawn that never bound, and any
    ///   resumed session that never became ready, before its deadline —
    ///   broadcasting the resulting [`SessionEvent::SpawnFailed`]s, so a launch
    ///   that crashed/hung (a fresh spawn before its first hook, or a
    ///   `claude --resume` that never reached `SessionStart(resume)`) can no
    ///   longer stall the UI on "pending" forever (the `SessionEnd` hook catches
    ///   the exited case immediately; this catches the hang-forever case).
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
                let now = std::time::Instant::now();
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
                match interactor.dispatch_ready_resumes(now).await {
                    Ok(dispatched_events) => {
                        for event in dispatched_events {
                            let _ = events.send(event);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "resume dispatch failed");
                    }
                }
                // Watchdog: reap fresh spawns that never bound and resumes that
                // never became ready before their deadlines. `Instant::now()` is
                // the live clock here; tests drive `reap_stale_spawns` directly
                // with an injected `now`.
                match interactor.reap_stale_spawns(now).await {
                    Ok(failed_events) => {
                        for event in failed_events {
                            let _ = events.send(event);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "spawn watchdog reap failed");
                    }
                }
                // Echo watchdog: release any send whose keystrokes vanished
                // without a trace — no echo, no turn boundary, no signal of any
                // kind — before its deadline, retrying it once and parking it
                // (held in the queue for the user to send or cancel) if that
                // retry is swallowed too. This is the one recovery that cannot
                // be event-driven, since the failure it covers produces no
                // event to react to; the ticks are what make the silence
                // observable. `Instant::now()` is the live clock here; tests
                // drive `sweep_echo_deadlines` directly with an injected `now`.
                match interactor.sweep_echo_deadlines(now).await {
                    Ok(dispatched_events) => {
                        for event in dispatched_events {
                            let _ = events.send(event);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "echo deadline sweep failed");
                    }
                }
                match interactor.poll_transcript().await {
                    Ok((groups, resolved_events)) => {
                        // One non-empty group per session that ingested new lines.
                        for messages in groups {
                            let session_id = messages[0].session_id.clone();
                            let thread_ids: Vec<_> = messages
                                .iter()
                                .map(|m| m.thread_id)
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect();
                            // A send error only means there are no subscribers;
                            // that is fine. We use the raw sender (not
                            // `broadcast`) because `&self` is not available inside
                            // the task.
                            let _ = events.send(SessionEvent::TranscriptUpdated {
                                session_id,
                                thread_ids,
                            });
                        }
                        // Permission-resolution events from the ingest (a late
                        // tool_result tailed in here): broadcast so the browser
                        // clears the "permission requested" notice.
                        for event in resolved_events {
                            let _ = events.send(event);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "transcript tail poll failed");
                    }
                }
            }
        })
    }
}
