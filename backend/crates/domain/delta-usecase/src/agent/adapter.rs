//! [`AgentAdapter`]: the trait the core drives every provider through.
//!
//! An adapter turns the neutral operations (launch, send, interrupt, close,
//! …) into whatever the concrete provider needs, and exposes the provider's
//! activity back as an [`AgentEvent`] stream. The core holds only this trait
//! plus [`AgentCapabilities`]; all provider-specific wire detail lives inside
//! the gateway adapter that implements it.
//!
//! The request/handle types here are intentionally minimal — just enough to
//! express the current usage. They carry provider-neutral fields; a provider's
//! own launch knobs are resolved into `extra_args` (or the provider's adapter
//! config) before they reach here.

use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::agent::{AgentCapabilities, AgentEvent, AgentProvider};
use crate::error::Result;

/// Inputs for launching a fresh agent session.
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    /// Delta's own conversation id. Providers that let Delta pin the identity
    /// (see [`super::SessionIdentityCapability::DeltaCanSetId`]) use it as the
    /// provider session id; others ignore it and return their own.
    pub session_id: String,
    /// The working directory the agent runs in.
    pub workdir: String,
    /// Provider-specific launch flags/fields, already resolved to argv tokens
    /// (Claude's launch-option flags). Empty when none were selected.
    pub extra_args: Vec<String>,
    /// A first prompt delivered at launch, when the session was started from
    /// the composer's first Send.
    pub first_prompt: Option<String>,
}

/// Inputs for resuming a previously-closed agent session.
#[derive(Debug, Clone)]
pub struct ResumeRequest {
    /// Delta's own conversation id.
    pub session_id: String,
    /// The provider's id for the session being resumed (Claude passes it to
    /// `--resume <id>`).
    pub provider_session_id: String,
    /// The working directory to resume the agent in.
    pub workdir: String,
}

/// Inputs for sending a user prompt into an open session.
#[derive(Debug, Clone)]
pub struct SendRequest {
    /// The visible prompt text, exactly as it should appear to the user. Any
    /// hidden per-turn context is injected out of band by the adapter
    /// (capability [`super::ContextInjectionCapability::HiddenPerTurn`]) and is
    /// deliberately NOT part of this text.
    pub text: String,
}

/// The outcome of a [`AgentAdapter::send`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendReceipt {
    /// The provider's id for the accepted prompt, when it returns one.
    pub provider_message_id: Option<String>,
}

/// A live handle to an open agent session.
///
/// Besides the provider identity it carries an opaque adapter-local `key` the
/// adapter uses to address the session's underlying resource (for Claude, the
/// tmux pane token). The core treats `key` as opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionHandle {
    pub provider: AgentProvider,
    pub provider_session_id: String,
    pub key: String,
}

/// A handle to an attachable terminal surface (Claude's tmux pane).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyHandle {
    /// The adapter-local terminal target (Claude's fully-qualified pane, e.g.
    /// `delta-1:0.0`).
    pub target: String,
}

/// The stream of [`AgentEvent`]s for one session.
///
/// A thin wrapper over an unbounded receiver, so callers depend on this crate's
/// surface rather than reaching for the channel type directly. Each session's
/// stream is handed out once; a second request yields an already-closed stream.
#[derive(Debug)]
pub struct AgentEventStream {
    rx: UnboundedReceiver<AgentEvent>,
}

impl AgentEventStream {
    /// Wrap a receiver as an event stream.
    pub fn new(rx: UnboundedReceiver<AgentEvent>) -> Self {
        Self { rx }
    }

    /// Receive the next event, or `None` once the session's sender is gone and
    /// the buffer is drained.
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        self.rx.recv().await
    }
}

/// The seam between Delta's core and a concrete agent provider.
///
/// Implementations live in the gateway layer. The core depends only on this
/// trait and the capability profile it reports; it never names a concrete
/// provider to decide behaviour.
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    /// Which provider this adapter drives.
    fn provider(&self) -> AgentProvider;

    /// The capability profile the core switches behaviour on.
    fn capabilities(&self) -> AgentCapabilities;

    /// Launch a fresh session.
    async fn launch(&self, req: LaunchRequest) -> Result<AgentSessionHandle>;

    /// Resume a previously-closed session.
    async fn resume(&self, req: ResumeRequest) -> Result<AgentSessionHandle>;

    /// Send a user prompt into an open session.
    async fn send(&self, handle: &AgentSessionHandle, req: SendRequest) -> Result<SendReceipt>;

    /// Interrupt the session's in-flight turn.
    async fn interrupt(&self, handle: &AgentSessionHandle) -> Result<()>;

    /// Close the session, tearing down its underlying resource.
    async fn close(&self, handle: &AgentSessionHandle) -> Result<()>;

    /// Take the session's event stream. Handed out once per session.
    fn events(&self, handle: &AgentSessionHandle) -> AgentEventStream;

    /// Attach to the session's terminal, when it has one. `Ok(None)` when the
    /// provider exposes no attachable terminal
    /// ([`super::TerminalCapability::NoTerminal`]).
    async fn attach_terminal(&self, handle: &AgentSessionHandle) -> Result<Option<PtyHandle>>;
}
