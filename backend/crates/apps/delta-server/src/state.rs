//! Shared application state.

use std::sync::Arc;

use tokio::sync::broadcast;

use delta_usecase::SessionEvent;
use delta_wire::{AppInteractor, Config};

/// Capacity of the per-process event broadcast channel.
const EVENT_CHANNEL_CAPACITY: usize = 256;

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
}
