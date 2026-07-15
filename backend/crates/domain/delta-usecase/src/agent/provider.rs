//! The set of AI-agent providers Delta can drive.

/// Which AI-agent backend a session is driven by.
///
/// The core never branches on this to decide behaviour — it consults
/// [`super::AgentCapabilities`] instead, so a new provider is a new capability
/// profile rather than a new `match` arm scattered across the code. The enum
/// exists so the provider can be recorded (per-session) and surfaced (a badge
/// in the UI), not so control flow can key off "is this Claude".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentProvider {
    /// Anthropic's Claude Code, driven via a tmux-hosted PTY plus HTTP hooks
    /// and a JSONL transcript tail.
    Claude,
    /// OpenAI's Codex, driven via `codex app-server` (stdio JSON-RPC).
    /// Reserved: no adapter ships in this phase.
    Codex,
}

impl AgentProvider {
    /// The stable wire token persisted for this provider (the value stored in
    /// the `session.provider` column, `'claude'` for the historical default).
    pub fn as_str(self) -> &'static str {
        match self {
            AgentProvider::Claude => "claude",
            AgentProvider::Codex => "codex",
        }
    }
}
