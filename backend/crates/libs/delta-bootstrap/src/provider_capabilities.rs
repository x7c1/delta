//! Each provider's static capability profile, read without a live adapter.

use claude_agent::CLAUDE_CAPABILITIES;
use codex_agent::CODEX_CAPABILITIES;
use delta_usecase::{AgentCapabilities, AgentProvider};

/// The static capability profile for a provider, resolved *without* a live
/// adapter instance.
///
/// Each provider's profile is declared once in its gateway adapter (the
/// `*_CAPABILITIES` const its [`AgentAdapter::capabilities`] returns) and read
/// back here through the same const, so the value the REST layer surfaces can
/// never drift from what a running adapter reports. The composition root is the
/// natural home: it is the one layer that already knows every gateway adapter,
/// and callers (e.g. `GET /api/providers`) need a provider's profile before —
/// or entirely without — an adapter being spawned.
///
/// Adding a provider is a new [`AgentProvider`] variant plus its capability
/// profile in the gateway layer plus a new arm here — the same fan-out the
/// availability probe documents.
///
/// [`AgentAdapter::capabilities`]: delta_usecase::AgentAdapter::capabilities
pub fn provider_capabilities(provider: AgentProvider) -> AgentCapabilities {
    match provider {
        AgentProvider::Claude => CLAUDE_CAPABILITIES,
        AgentProvider::Codex => CODEX_CAPABILITIES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The static accessor resolves each provider's terminal capability without
    /// a live adapter: Claude offers an attachable PTY, Codex has no terminal.
    /// This is the fact the workspace's terminal gating hangs on — a Codex
    /// session must never show a terminal tab.
    #[test]
    fn provider_capabilities_report_the_terminal_surface_per_provider() {
        use delta_usecase::TerminalCapability;

        assert_eq!(
            provider_capabilities(AgentProvider::Claude).terminal,
            TerminalCapability::AttachablePty,
        );
        assert_eq!(
            provider_capabilities(AgentProvider::Codex).terminal,
            TerminalCapability::NoTerminal,
        );
    }

    /// The accessor returns exactly what each adapter's `capabilities()` returns
    /// — the guarantee that the REST-surfaced profile can never drift from a
    /// running adapter's. Asserted against the adapter consts directly (both are
    /// the single source of truth the accessor reads).
    #[test]
    fn provider_capabilities_match_the_adapter_source_of_truth() {
        assert_eq!(
            provider_capabilities(AgentProvider::Claude),
            CLAUDE_CAPABILITIES,
        );
        assert_eq!(
            provider_capabilities(AgentProvider::Codex),
            CODEX_CAPABILITIES,
        );
    }
}
