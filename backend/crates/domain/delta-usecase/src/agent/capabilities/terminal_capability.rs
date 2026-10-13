//! What kind of terminal surface the provider needs or offers.

/// What kind of terminal surface the provider needs or offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCapability {
    /// A live PTY the browser can attach to (Claude's tmux pane).
    AttachablePty,
    /// The provider runs a process but exposes no terminal worth attaching.
    NoPtyNeeded,
    /// No terminal at all (Codex app-server).
    NoTerminal,
}
