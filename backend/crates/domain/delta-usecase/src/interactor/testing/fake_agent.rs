//! An in-memory fake [`AgentAdapter`] + [`AgentAdapterFactory`] for the
//! terminal-less (Codex) session-creation use-case tests.
//!
//! The real Codex adapter lives in the `codex-agent` gateway crate, which the
//! domain must not depend on, so the interactor unit tests drive this stand-in
//! instead. It records what it was asked to do (launch/send) and returns a
//! scripted provider thread id + turn id, so a test can assert the persistence
//! and turn-FSM behaviour of a Codex spawn without a real `codex app-server`.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::agent::{
    AgentAdapter, AgentAdapterFactory, AgentCapabilities, AgentEventStream, AgentProvider,
    AgentSessionHandle, ContextInjectionCapability, EventCapability, ForkCapability,
    InterruptCapability, LaunchCapability, LaunchRequest, PermissionCapability, PtyHandle,
    ResumeCapability, ResumeRequest, SendReceipt, SendRequest, SessionIdentityCapability,
    SteerCapability, TerminalCapability, TranscriptCapability,
};
use crate::error::{Error, Result};

/// What the fake adapter observed, for a test to inspect after a spawn.
#[derive(Debug, Default, Clone)]
pub(crate) struct FakeAgentLog {
    /// The `LaunchRequest`s the adapter received, in order.
    pub launches: Vec<LaunchRequest>,
    /// The visible send texts the adapter received, in order.
    pub sends: Vec<String>,
    /// The number of `close` calls.
    pub closes: usize,
}

/// A scripted, in-memory [`AgentAdapter`] standing in for the Codex adapter.
pub(crate) struct FakeAgentAdapter {
    /// The provider thread id the fake mints at launch (the provider session id
    /// + handle key). Session ↔ thread is 1:1, like Codex.
    thread_id: String,
    /// The turn id returned from each `send` as the receipt's provider message
    /// id. `None` reproduces a `turn/start` ack that carried no turn id.
    turn_id: Option<String>,
    log: Arc<Mutex<FakeAgentLog>>,
}

impl FakeAgentAdapter {
    fn new(thread_id: String, turn_id: Option<String>, log: Arc<Mutex<FakeAgentLog>>) -> Arc<Self> {
        Arc::new(Self {
            thread_id,
            turn_id,
            log,
        })
    }
}

#[async_trait]
impl AgentAdapter for FakeAgentAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Codex
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            launch: LaunchCapability::JsonRpcAppServer,
            session_identity: SessionIdentityCapability::ProviderReturnsId,
            resume: ResumeCapability::Supported,
            events: EventCapability::StructuredTurnEvents,
            transcript: TranscriptCapability::EventStreamOnly,
            permission: PermissionCapability::AdapterDecision,
            context_injection: ContextInjectionCapability::HiddenPerTurn,
            interrupt: InterruptCapability::Rpc,
            terminal: TerminalCapability::NoTerminal,
            fork: ForkCapability::None,
            steer: SteerCapability::None,
        }
    }

    async fn launch(&self, req: LaunchRequest) -> Result<AgentSessionHandle> {
        self.log.lock().unwrap().launches.push(req);
        Ok(AgentSessionHandle {
            provider: AgentProvider::Codex,
            provider_session_id: self.thread_id.clone(),
            key: self.thread_id.clone(),
        })
    }

    async fn resume(&self, req: ResumeRequest) -> Result<AgentSessionHandle> {
        Ok(AgentSessionHandle {
            provider: AgentProvider::Codex,
            provider_session_id: req.provider_session_id.clone(),
            key: req.provider_session_id,
        })
    }

    async fn send(&self, _handle: &AgentSessionHandle, req: SendRequest) -> Result<SendReceipt> {
        self.log.lock().unwrap().sends.push(req.text);
        Ok(SendReceipt {
            provider_message_id: self.turn_id.clone(),
        })
    }

    async fn interrupt(&self, _handle: &AgentSessionHandle) -> Result<()> {
        Ok(())
    }

    async fn close(&self, _handle: &AgentSessionHandle) -> Result<()> {
        self.log.lock().unwrap().closes += 1;
        Ok(())
    }

    fn events(&self, _handle: &AgentSessionHandle) -> AgentEventStream {
        // No live pump in this slice; hand out an already-closed stream.
        let (_tx, rx) = mpsc::unbounded_channel();
        AgentEventStream::new(rx)
    }

    async fn attach_terminal(&self, _handle: &AgentSessionHandle) -> Result<Option<PtyHandle>> {
        Ok(None)
    }
}

/// How the fake factory behaves when `connect` is called.
enum ConnectOutcome {
    /// Build the scripted adapter.
    Adapter {
        thread_id: String,
        turn_id: Option<String>,
    },
    /// Fail the connection (e.g. Codex not installed), for rollback tests.
    Fail,
}

/// A [`AgentAdapterFactory`] that hands out a [`FakeAgentAdapter`] (or fails).
pub(crate) struct FakeAgentFactory {
    outcome: ConnectOutcome,
    log: Arc<Mutex<FakeAgentLog>>,
}

impl FakeAgentFactory {
    /// A factory whose `connect` yields an adapter minting `thread_id` and
    /// returning `turn_id` from each send.
    pub(crate) fn new(thread_id: impl Into<String>, turn_id: Option<&str>) -> Arc<Self> {
        Arc::new(Self {
            outcome: ConnectOutcome::Adapter {
                thread_id: thread_id.into(),
                turn_id: turn_id.map(str::to_owned),
            },
            log: Arc::new(Mutex::new(FakeAgentLog::default())),
        })
    }

    /// A factory whose `connect` fails, so a caller must roll back cleanly.
    pub(crate) fn failing() -> Arc<Self> {
        Arc::new(Self {
            outcome: ConnectOutcome::Fail,
            log: Arc::new(Mutex::new(FakeAgentLog::default())),
        })
    }

    /// The observation log the built adapter writes to. For the failing factory
    /// this stays empty (no adapter is built).
    pub(crate) fn log(&self) -> Arc<Mutex<FakeAgentLog>> {
        Arc::clone(&self.log)
    }
}

#[async_trait]
impl AgentAdapterFactory for FakeAgentFactory {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Codex
    }

    async fn connect(&self) -> Result<Arc<dyn AgentAdapter>> {
        match &self.outcome {
            ConnectOutcome::Adapter { thread_id, turn_id } => {
                // Share the factory's log with the adapter, so `factory.log()`
                // reflects the built adapter's live observations.
                let adapter = FakeAgentAdapter::new(
                    thread_id.clone(),
                    turn_id.clone(),
                    Arc::clone(&self.log),
                );
                Ok(adapter as Arc<dyn AgentAdapter>)
            }
            ConnectOutcome::Fail => Err(Error::Agent(
                "fake codex connect failed (provider unavailable)".to_owned(),
            )),
        }
    }
}
