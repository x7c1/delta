//! Shared application state.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;

use delta_usecase::SessionEvent;
use delta_wire::{AppInteractor, Config};

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
    tmux_pane: String,
}

impl AppState {
    /// Build the shared state from configuration, wiring the Interactor.
    pub fn build(config: &Config) -> anyhow::Result<Self> {
        let interactor = delta_wire::build(config)?;
        Ok(Self::from_interactor(
            interactor,
            config.tmux_pane.clone(),
        ))
    }

    /// Build the shared state from an already-wired Interactor.
    ///
    /// The Interactor's gateways are type-erased (see [`AppInteractor`]), so
    /// integration tests can inject fakes (an in-memory store, a temp-file
    /// transcript, a no-op tmux driver) and still produce this exact
    /// [`AppState`] type — no generics leak into the transport layer.
    pub fn from_interactor(interactor: AppInteractor, tmux_pane: String) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            interactor: Arc::new(interactor),
            events,
            tmux_pane,
        }
    }

    /// The wired Interactor.
    pub fn interactor(&self) -> &AppInteractor {
        &self.interactor
    }

    /// The tmux pane the session lives in (used by the PTY bridge).
    pub fn tmux_pane(&self) -> &str {
        &self.tmux_pane
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
    /// Every [`TRANSCRIPT_POLL_INTERVAL`], poll the registered session's
    /// transcript for newly-written lines. When any were ingested, broadcast a
    /// [`SessionEvent::TranscriptUpdated`] carrying the distinct threads they
    /// landed on so browsers refetch them. This catches the assistant reply that
    /// Claude Code flushes to the JSONL *after* the `Stop` hook fires, which the
    /// hook sync misses.
    ///
    /// The task clones the `Arc`-shared interactor and the broadcast sender, so
    /// it stays alive independently of any request. A poll error is logged and
    /// the loop continues — a transient read failure must never kill the tail.
    pub fn spawn_transcript_tail(&self) -> tokio::task::JoinHandle<()> {
        let interactor = Arc::clone(&self.interactor);
        let events = self.events.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(TRANSCRIPT_POLL_INTERVAL);
            loop {
                ticker.tick().await;
                match interactor.poll_transcript().await {
                    Ok(messages) if messages.is_empty() => {}
                    Ok(messages) => {
                        let session_id = messages[0].session_id.clone();
                        let thread_ids: Vec<_> = messages
                            .iter()
                            .map(|m| m.thread_id)
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect();
                        // A send error only means there are no subscribers; that
                        // is fine. We use the raw sender (not `broadcast`) because
                        // `&self` is not available inside the task.
                        let _ = events.send(SessionEvent::TranscriptUpdated {
                            session_id,
                            thread_ids,
                        });
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "transcript tail poll failed");
                    }
                }
            }
        })
    }
}
