//! A live, terminal-less agent session.

use std::sync::Arc;

use crate::agent::{AgentAdapter, AgentSessionHandle};

#[cfg(doc)]
use super::{OpenHandle, SessionRuntime};

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
/// pane-bound path is untouched: [`SessionRuntime::handle`] stays `None` and no
/// spawn is pending, so the PTY bridge's lookup resolves nothing and refuses to
/// attach — a Codex
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
