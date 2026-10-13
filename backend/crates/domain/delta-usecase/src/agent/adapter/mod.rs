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
//!
//! ## The worktree's source repository
//!
//! [`LaunchRequest::worktree_repo_root`] / [`ResumeRequest::worktree_repo_root`]
//! carry one fact about the session's working directory that a provider cannot
//! be expected to work out: that Delta created it as a **linked git worktree**,
//! and which repository it was cut from.
//!
//! It matters because a linked worktree's `.git` is only a pointer file
//! (`gitdir: <repo-root>/.git/worktrees/<id>`) — git's actual writes land in the
//! source repository's `.git` (the per-worktree `worktrees/<id>/index` plus the
//! shared `objects/` and `refs/`), which is outside the working directory
//! entirely. A provider that sandboxes writes to the area around its working
//! directory therefore blocks — or raises an approval prompt for — every
//! routine `git add` / `git commit` in such a session, unless it is told about
//! that directory. Delta is the party that created the worktree, so Delta is
//! the party that knows; it is derived fresh on every launch and resume and
//! registered nowhere.
//!
//! Adapters whose providers do not sandbox writes ignore it: the Claude launch
//! is unaffected by this field.
//!
//! One limit is worth knowing, because it is reachable rather than exotic: the
//! value is the *working-tree top* of the directory the session was started
//! from (`git rev-parse --show-toplevel`), and nothing constrains that directory
//! to be a main clone — the picker browses the filesystem freely, and its
//! "recent" list carries every directory a past session was started from, a
//! linked worktree the user maintains by hand included. Start a worktree session
//! from a directory that is itself a linked worktree and the root named here is
//! that worktree, whose own `.git` is again a pointer file, while git still
//! writes through the original clone's. Such a session keeps the approval
//! prompts, exactly as before this field existed. Naming the repository's
//! *common* git directory (`git rev-parse --git-common-dir`, which resolves to
//! the original clone from anywhere) is what would close that case.

use async_trait::async_trait;

use crate::agent::{AgentCapabilities, AgentContentSource, AgentProvider, NullContentSource};
use crate::error::{Error, Result};
use crate::interactor::PermissionDecision;

#[cfg(doc)]
use crate::AgentEvent;

mod agent_event_stream;
pub use agent_event_stream::AgentEventStream;

mod agent_session_handle;
pub use agent_session_handle::AgentSessionHandle;

mod content_source_request;
pub use content_source_request::ContentSourceRequest;

mod launch_option_spec;
pub use launch_option_spec::LaunchOptionSpec;

mod launch_request;
pub use launch_request::LaunchRequest;

mod pty_handle;
pub use pty_handle::PtyHandle;

mod resume_request;
pub use resume_request::ResumeRequest;

mod send_receipt;
pub use send_receipt::SendReceipt;

mod send_request;
pub use send_request::SendRequest;

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
    /// session — Codex reads the model the server resolved for the thread off its
    /// `thread/start` / `thread/resume` response and stamps it on the session's
    /// messages, which is the only truthful source for it (the model may come
    /// from a launch option, the user's own Codex config, or the server's
    /// default, and only the response says which won).
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
