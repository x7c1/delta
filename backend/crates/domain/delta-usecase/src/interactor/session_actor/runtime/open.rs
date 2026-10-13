//! The open-session state: the bound tmux pane ([`OpenHandle`]) for a
//! pane-backed provider, and its terminal-less parallel ([`OpenAgentSession`])
//! for an adapter-backed one, with the bind/remove lifecycle for both.

use super::SessionRuntime;
use super::{OpenAgentSession, OpenHandle};

#[cfg(doc)]
use crate::agent::AgentAdapter;

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
