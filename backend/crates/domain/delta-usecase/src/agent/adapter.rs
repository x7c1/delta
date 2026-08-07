//! [`AgentAdapter`]: the trait the core drives every provider through.
//!
//! An adapter turns the neutral operations (launch, send, interrupt, close,
//! …) into whatever the concrete provider needs, and exposes the provider's
//! activity back as an [`AgentEvent`] stream. The core holds only this trait
//! plus [`AgentCapabilities`]; all provider-specific wire detail lives inside
//! the gateway adapter that implements it.
//!
//! The request/handle types here are intentionally minimal — just enough to
//! express the current usage. They carry provider-neutral fields; the user's
//! selected launch options arrive as neutral [`LaunchOptionSpec`] records that
//! each adapter renders in its own way (Claude → argv flags, Codex →
//! `thread/start` fields), so no caller in the core has to know which shape a
//! provider wants.

use async_trait::async_trait;
use delta_model::{SessionId, ThreadId};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::agent::{
    AgentCapabilities, AgentContentSource, AgentEvent, AgentProvider, NullContentSource,
};
use crate::error::{Error, Result};
use crate::interactor::PermissionDecision;

/// One launch option the user selected for this session, resolved from the
/// registry into the neutral `(name, value?)` pair the registry stores.
///
/// The pair is deliberately **not** rendered here: what a `name` means is a
/// provider concern, and rendering it is the adapter's job. Claude reads the
/// pair as a CLI flag and its argument (`--model opus`, see [`Self::to_argv`]);
/// Codex reads it as a `thread/start` field name and its value (`model` →
/// `"gpt-5.6-sol"`). Keeping the pair neutral is what lets the core hand every
/// provider the same list instead of branching on which provider it is talking
/// to.
///
/// Values arrive already resolved (a leading `~` is expanded by the core, since
/// no shell ever runs over a launch-option value for any provider), so an
/// adapter renders them verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptionSpec {
    /// What the option is called — a CLI flag for an argv-launched provider
    /// (`--permission-mode`), a request field name for a structured one
    /// (`sandbox`).
    pub name: String,
    /// The option's argument/value; `None` for a valueless option.
    pub value: Option<String>,
}

impl LaunchOptionSpec {
    /// Render this option as argv tokens, the shape a command-line-launched
    /// provider needs: the name, followed by the value when there is one (a
    /// valueless option contributes only its name).
    pub fn to_argv(&self) -> Vec<String> {
        match &self.value {
            Some(value) => vec![self.name.clone(), value.clone()],
            None => vec![self.name.clone()],
        }
    }
}

/// Inputs for launching a fresh agent session.
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    /// Delta's own conversation id. Providers that let Delta pin the identity
    /// (see [`super::SessionIdentityCapability::DeltaCanSetId`]) use it as the
    /// provider session id; others ignore it and return their own.
    pub session_id: String,
    /// The working directory the agent runs in.
    pub workdir: String,
    /// The launch options the user selected, in selection order, as neutral
    /// `(name, value?)` pairs. The adapter renders them for its provider —
    /// Claude appends them to the launch argv, Codex maps them onto
    /// `thread/start` fields. Empty when none were selected.
    pub launch_options: Vec<LaunchOptionSpec>,
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

/// Inputs for building one session's push-based content accumulator
/// ([`AgentAdapter::content_source`]).
///
/// Everything here is a **per-session** fact: known once the session is bound
/// and constant for its lifetime. That is why the accumulator takes them at
/// construction rather than being told them again for every turn — the
/// *per-turn* routing context travels separately, through
/// [`AgentContentSource::begin_turn`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSourceRequest {
    /// Delta's own conversation id, stamped on every message the source folds.
    pub session_id: SessionId,
    /// The session's `main` thread: where a plain turn's messages land, and
    /// what each turn's routing resets to.
    pub main_thread: ThreadId,
    /// The first `seq` to mint — the store's current `MAX(seq) + 1`, so minted
    /// ordering continues past whatever is already persisted (`0` for a fresh
    /// session, the persisted message count on a resume).
    pub seed_seq: i64,
    /// The directory the session's agent runs in: the launch directory Delta
    /// resolved at spawn and recorded on the session row (the git worktree path
    /// when the session was started with one). Taken from Delta's own record
    /// rather than re-derived, so a message's `cwd` always agrees with the
    /// session's other launch-site columns.
    pub cwd: String,
    /// The branch the session launched on, as recorded on the session row
    /// (`branch_at_launch`). `None` when Delta recorded none — a session started
    /// without a git worktree leaves that column NULL — so the fact degrades
    /// rather than being invented.
    pub git_branch: Option<String>,
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

    /// Inject hidden per-turn context into an open session — text the model
    /// sees on its next turn without it appearing in the visible prompt
    /// (capability [`super::ContextInjectionCapability::HiddenPerTurn`]).
    ///
    /// The core calls this before dispatching a **branch send** (branch from
    /// selected text): the branched-from passage is delivered here as hidden
    /// context so the branch turn is anchored to it, while the visible prompt
    /// stays exactly what the user typed.
    ///
    /// The default is an error, so reaching it is a wiring mistake surfaced
    /// rather than silently dropped. Claude does **not** route here: it injects
    /// hidden context through its own `UserPromptSubmit` hook `additionalContext`
    /// path, unchanged by this method. Codex overrides this with
    /// `thread/inject_items`.
    async fn inject_context(&self, _handle: &AgentSessionHandle, _text: &str) -> Result<()> {
        Err(Error::Agent(format!(
            "the {:?} adapter does not inject hidden context over this trait method",
            self.provider()
        )))
    }

    /// Interrupt the session's in-flight turn.
    async fn interrupt(&self, handle: &AgentSessionHandle) -> Result<()>;

    /// Answer a pending permission request with a decision, over the provider's
    /// wire.
    ///
    /// `request_id` is the adapter-scoped provider token the adapter surfaced on
    /// the matching [`AgentEvent::PermissionRequested`]
    /// ([`AgentPermissionRequest::request_id`]): the core stores it verbatim
    /// alongside the Delta permission-row id and hands it back here, never
    /// interpreting it. The adapter owns the whole wire translation — mapping the
    /// neutral [`PermissionDecision`] onto the provider's own decision value and
    /// answering the outstanding request — and emits an
    /// [`AgentEvent::PermissionResolved`] on the session's stream so the core's
    /// event pump can settle its runtime mirror and browser notice.
    ///
    /// The default is an error: a provider whose permission decisions are
    /// *ingested* rather than *answered over the wire* (Claude, which resolves a
    /// dialog through its hook + transcript path) never routes a decision here,
    /// so reaching this default is a wiring mistake, surfaced rather than
    /// silently dropped.
    ///
    /// [`AgentPermissionRequest::request_id`]: crate::agent::AgentPermissionRequest::request_id
    async fn resolve_permission(
        &self,
        _handle: &AgentSessionHandle,
        _request_id: &str,
        _decision: PermissionDecision,
    ) -> Result<()> {
        Err(Error::Agent(format!(
            "the {:?} adapter does not answer permission decisions over the wire \
             (its decisions are ingested through the hook/transcript path)",
            self.provider()
        )))
    }

    /// Close the session, tearing down its underlying resource.
    async fn close(&self, handle: &AgentSessionHandle) -> Result<()>;

    /// Take the session's event stream. Handed out once per session.
    fn events(&self, handle: &AgentSessionHandle) -> AgentEventStream;

    /// Build the push-based content accumulator for one session's event stream.
    ///
    /// The event pump feeds this every [`AgentEvent`] the session's [`events`]
    /// stream yields and persists the canonical content each event completes. It
    /// is a provider concern — how a provider's structured frames fold into
    /// Delta's canonical messages — so it lives on the adapter, keyed by the
    /// session's identity and launch site ([`ContentSourceRequest`]).
    ///
    /// `handle` names the session on the *provider's* side, so an adapter can
    /// join the neutral request with whatever it learned when it opened that
    /// session — Codex reads the model the server resolved for the thread off
    /// its `thread/start` / `thread/resume` response and stamps it on the
    /// session's messages, which is the only truthful source for it (the model
    /// may come from a launch option, the user's own Codex config, or the
    /// server's default).
    ///
    /// The default returns a [`NullContentSource`]: a provider that pulls its
    /// content from a transcript (Claude) rather than pushing structured frames
    /// runs no such pump, so it needs no accumulator. A push-based provider
    /// (Codex) overrides this with a real accumulator.
    ///
    /// [`events`]: Self::events
    fn content_source(
        &self,
        _handle: &AgentSessionHandle,
        _req: ContentSourceRequest,
    ) -> Box<dyn AgentContentSource> {
        Box::new(NullContentSource)
    }

    /// Attach to the session's terminal, when it has one. `Ok(None)` when the
    /// provider exposes no attachable terminal
    /// ([`super::TerminalCapability::NoTerminal`]).
    async fn attach_terminal(&self, handle: &AgentSessionHandle) -> Result<Option<PtyHandle>>;
}
