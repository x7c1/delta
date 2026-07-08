//! Shared application state.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;

use delta_bootstrap::{AppInteractor, Config};
use delta_usecase::{SessionEvent, SessionLifecycle};

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
    /// Delta's dedicated tmux socket, so the PTY bridge attaches on the same
    /// server the sessions live on (`tmux -L <socket> attach-session`).
    tmux_socket: Arc<str>,
}

impl AppState {
    /// Build the shared state from configuration, wiring the Interactor.
    ///
    /// Async because the composition root's boot-time send reconcile (the
    /// sweep returning restart-orphaned `dispatched` rows to `queued`) runs
    /// against the freshly-opened store before the state is handed out.
    pub async fn build(config: &Config) -> anyhow::Result<Self> {
        let interactor = delta_bootstrap::build(config).await?;
        Ok(Self::from_interactor(interactor, &config.tmux_socket))
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
        Self {
            interactor: Arc::new(interactor),
            events,
            tmux_socket: Arc::from(tmux_socket),
        }
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

    /// Spawn the continuous transcript tail.
    ///
    /// Every [`TRANSCRIPT_POLL_INTERVAL`], poll every registered session's
    /// transcript for newly-written lines. For each session that ingested new
    /// lines, broadcast a [`SessionEvent::TranscriptUpdated`] carrying the
    /// distinct threads they landed on so browsers refetch them. This catches the
    /// assistant reply that Claude Code flushes to the JSONL *after* the `Stop`
    /// hook fires, which the hook sync misses.
    ///
    /// The same tick also runs two registry sweeps that must execute outside any
    /// hook handler:
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
    ///
    /// Both sweeps share this loop rather than owning their own tasks — all are
    /// cheap periodic passes over the same registry.
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
