//! Which AI-agent backend a session is driven by.

use crate::error::{Error, Result};

/// Which AI-agent backend a session is driven by.
///
/// This is recorded per session (persisted in the `session.provider` column)
/// and surfaced in the UI (a provider badge), *not* used to branch control
/// flow: the core keys behaviour off the provider's capabilities rather than
/// off "is this Claude". A new provider is therefore a new value here plus a
/// capability profile in the gateway layer, not a new `match` arm scattered
/// through the domain.
///
/// It lives in `delta-model` (alongside [`crate::SessionStatus`]) because it is
/// a persisted field of the [`crate::Session`] entity; the provider-neutral
/// agent contract in `delta-usecase` re-exports it so the contract still refers
/// to a single `AgentProvider` type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentProvider {
    /// Anthropic's Claude Code, driven via a tmux-hosted PTY plus HTTP hooks
    /// and a JSONL transcript tail. The historical default: every session
    /// Delta launched before multi-provider support is `Claude`.
    Claude,
    /// OpenAI's Codex, driven via `codex app-server` (stdio JSON-RPC).
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

    /// Parse the persisted wire token back into a provider. Mirrors
    /// [`Self::as_str`]; an unknown token is an [`Error::InvalidVariant`].
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "claude" => Ok(AgentProvider::Claude),
            "codex" => Ok(AgentProvider::Codex),
            other => Err(Error::InvalidVariant {
                kind: "AgentProvider",
                value: other.to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_enum_round_trips() {
        for p in [AgentProvider::Claude, AgentProvider::Codex] {
            assert_eq!(AgentProvider::parse(p.as_str()).unwrap(), p);
        }
    }

    #[test]
    fn an_unknown_token_is_rejected() {
        assert!(AgentProvider::parse("gemini").is_err());
    }
}
