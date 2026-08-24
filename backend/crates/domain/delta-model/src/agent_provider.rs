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
    /// Every provider Delta knows, in declaration order.
    ///
    /// Exists so a caller that must do something *for each* provider — read
    /// every provider's capability profile, reconcile every provider's shipped
    /// launch options — iterates one shared list instead of writing its own,
    /// which is how such lists quietly fall out of step with each other. This is
    /// still a hand-written list, and nothing makes the compiler verify that it
    /// is complete — a new variant left out of it would compile. What guards it
    /// is position: it sits immediately above [`Self::as_str`], whose `match` is
    /// over `Self` and therefore exhaustive, so a new variant stops the compiler
    /// a few lines below and the author is already editing here. ([`Self::parse`]
    /// matches on the wire token rather than on the variant, so it catches
    /// nothing.)
    pub const ALL: [AgentProvider; 2] = [AgentProvider::Claude, AgentProvider::Codex];

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
        for p in AgentProvider::ALL {
            assert_eq!(AgentProvider::parse(p.as_str()).unwrap(), p);
        }
    }

    /// [`AgentProvider::ALL`] lists each variant exactly once — the property
    /// every "for each provider" caller leans on. Spelled out as the expected
    /// tokens rather than derived from `ALL` itself, so the list has to be
    /// updated deliberately.
    #[test]
    fn all_lists_every_provider_once() {
        let mut tokens: Vec<&str> = AgentProvider::ALL.iter().map(|p| p.as_str()).collect();
        tokens.sort_unstable();
        assert_eq!(tokens, vec!["claude", "codex"]);
    }

    #[test]
    fn an_unknown_token_is_rejected() {
        assert!(AgentProvider::parse("gemini").is_err());
    }
}
