//! How the provider's agent process is launched.

/// How the provider's agent process is launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchCapability {
    /// Spawn a command in a PTY (Claude Code in a tmux pane).
    PtyCommand,
    /// Connect to a long-lived JSON-RPC app-server over stdio (Codex).
    JsonRpcAppServer,
}

impl LaunchCapability {
    /// Whether a provider with this launch capability runs its sessions through
    /// an [`AgentAdapter`] binding — terminal-less, no tmux pane, no hooks, no
    /// transcript file — rather than the native PTY path (Claude's tmux pane +
    /// HTTP hooks + JSONL transcript tail).
    ///
    /// This is the single dispatch predicate the session paths branch on:
    /// [`PtyCommand`](Self::PtyCommand) is the native path, every other launch
    /// capability is adapter-backed. A new structured provider therefore takes
    /// the adapter path by declaring its (non-PTY) launch capability, with no
    /// new `match` arm in the core.
    ///
    /// [`AgentAdapter`]: crate::AgentAdapter
    pub fn is_adapter_backed(self) -> bool {
        !matches!(self, LaunchCapability::PtyCommand)
    }
}
