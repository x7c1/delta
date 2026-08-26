//! The open-session state: the bound tmux pane ([`OpenHandle`]) for a
//! pane-backed provider, and its terminal-less parallel ([`OpenAgentSession`])
//! for an adapter-backed one, with the bind/remove lifecycle for both.

use std::sync::Arc;

use crate::agent::{AgentAdapter, AgentSessionHandle};
use crate::pane_token::PaneToken;

use super::SessionRuntime;

/// A live, bound session: its Claude `session_id` is known and it is mapped to
/// the tmux pane driving it.
#[derive(Debug, Clone)]
pub struct OpenHandle {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to and the PTY attaches to (`<token>:0.0`).
    pub pane: String,
}

/// A live, terminal-less agent session (e.g. Codex over `codex app-server`).
///
/// The parallel of [`OpenHandle`] for a provider that has no tmux pane: it
/// carries the live [`AgentAdapter`] and the provider's session handle instead
/// of a pane token. Holding the adapter here is what keeps the underlying
/// `codex app-server` connection alive for the session's lifetime — dropping it
/// (e.g. on actor retirement) would tear the connection down — so a session
/// with an `open_agent` reads as *open* and its actor never retires while it
/// exists (see [`SessionRuntime::is_empty`]).
///
/// There is deliberately no [`OpenHandle`] for such a session, so Claude's
/// pane-bound path is untouched: [`SessionRuntime::handle`] (the PTY routing
/// key) stays `None`, and the PTY bridge therefore refuses to attach — a Codex
/// session has nothing to attach to ([`crate::agent::TerminalCapability::NoTerminal`]).
#[derive(Clone)]
pub struct OpenAgentSession {
    /// The live adapter driving the provider. Kept alive here for the session's
    /// lifetime so its backing connection is not dropped underneath it.
    pub adapter: Arc<dyn AgentAdapter>,
    /// The provider's handle for this session (its provider session id + the
    /// adapter-local key), used to address the session on the adapter.
    pub handle: AgentSessionHandle,
}

impl std::fmt::Debug for OpenAgentSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The adapter is a trait object with no `Debug`; print the handle and
        // the provider it drives, which is the identifying state anyway.
        f.debug_struct("OpenAgentSession")
            .field("provider", &self.adapter.provider())
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

impl SessionRuntime {
    /// Whether a pane is live: bound to the session, spawned and awaiting its
    /// first `UserPromptSubmit`, or accepted with its launch preparation still
    /// running. Used to keep the single-session cold start idempotent — an
    /// accepted-but-not-yet-launched session has to count, or a second
    /// `POST /api/sessions` arriving while the first one's worktree is still
    /// being checked out would start a rival session. A terminal-less agent
    /// session also counts as live so the same check does not spawn a pane
    /// alongside an open Codex session.
    pub fn has_live_pane(&self) -> bool {
        self.open.is_some()
            || self.launching_spawn.is_some()
            || self.pending_spawn.is_some()
            || self.open_agent.is_some()
    }

    /// The open **pane** handle, if the session is currently open on a
    /// pane-backed provider. Always `None` for a terminal-less agent session,
    /// which is exactly what makes the PTY bridge refuse to attach to a Codex
    /// session (it has no pane).
    pub fn handle(&self) -> Option<&OpenHandle> {
        self.open.as_ref()
    }

    /// Whether the session is currently open — a live, bound pane (Claude) or a
    /// live terminal-less agent session (Codex).
    pub fn is_open(&self) -> bool {
        self.open.is_some() || self.open_agent.is_some()
    }

    /// Mark the session open on a terminal-less agent (Codex), holding its live
    /// adapter and handle. The pane-backed [`Self::open`] slot is left `None`.
    pub fn bind_agent(&mut self, agent: OpenAgentSession) {
        self.open_agent = Some(agent);
    }

    /// The live terminal-less agent session (Codex), when open — its adapter and
    /// provider handle. `None` for a pane-backed (Claude) or closed session.
    /// Read by the browser-decision path to reach [`AgentAdapter::resolve_permission`].
    ///
    /// [`AgentAdapter::resolve_permission`]: crate::agent::AgentAdapter::resolve_permission
    pub fn open_agent(&self) -> Option<&OpenAgentSession> {
        self.open_agent.as_ref()
    }

    /// Remove the terminal-less agent session (closing it), returning it so the
    /// caller can drive the adapter's `close`.
    ///
    /// Also drops the session's content accumulator and any permission
    /// correlations: both only have meaning while the agent session is open (its
    /// event pump ends when the adapter closes), so keeping them past the close
    /// would leak per-session state.
    pub fn remove_open_agent(&mut self) -> Option<OpenAgentSession> {
        self.agent_content_source = None;
        self.agent_permission_tokens.clear();
        self.open_agent.take()
    }

    /// Bind a pane to the session (the session is now open).
    pub fn bind(&mut self, handle: OpenHandle) {
        self.open = Some(handle);
    }

    /// Remove the bound pane (closing the session), returning its handle.
    pub fn remove_open(&mut self) -> Option<OpenHandle> {
        self.open.take()
    }
}
