//! The wire form of the provider-availability listing (`GET /api/providers`).

use delta_model::ProviderAvailability;
use delta_usecase::{AgentCapabilities, TerminalCapability};
use serde::Serialize;
use ts_rs::TS;

use crate::session::WireAgentProvider;

/// The UI-relevant slice of a provider's capability profile.
///
/// A *curated* projection of the internal [`AgentCapabilities`]: it carries only
/// the capabilities the frontend acts on today, not the whole contract (which
/// names launch/permission/transcript concepts the UI never branches on). The
/// UI keys behaviour off these flags rather than off the provider name — a
/// provider with no terminal hides the terminal tab whatever it is called.
///
/// Kept as a struct (rather than a bare flag on the parent) so a further
/// UI-relevant capability can join it without reshaping the response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "ProviderCapabilities")]
pub struct WireProviderCapabilities {
    /// Whether the provider offers a terminal the browser can attach to.
    /// Derived from the internal [`TerminalCapability`]:
    /// [`TerminalCapability::AttachablePty`] (Claude's tmux pane) → `true`;
    /// [`TerminalCapability::NoPtyNeeded`] / [`TerminalCapability::NoTerminal`]
    /// (Codex's headless app-server) → `false`. The workspace hides the terminal
    /// toggle and pane for a provider whose value is `false`.
    pub has_terminal: bool,
}

impl From<AgentCapabilities> for WireProviderCapabilities {
    fn from(capabilities: AgentCapabilities) -> Self {
        WireProviderCapabilities {
            has_terminal: matches!(capabilities.terminal, TerminalCapability::AttachablePty),
        }
    }
}

/// JSON shape of one provider's launch availability plus its capability profile.
///
/// `provider`/`available`/`detail` mirror the domain [`ProviderAvailability`]:
/// `available` reports whether the provider's launch binary is present on the
/// server host (v1 checks binary presence only); `detail` carries a
/// human-readable reason when it is not, which the new-session selector shows
/// next to the disabled option. The `detail`-carrying shape leaves room for a
/// future version-compatibility verdict without a breaking reshape (see the
/// domain type's docs). `capabilities` carries the UI-relevant capability
/// profile — resolved statically per provider, so it is present even for an
/// unavailable provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "ProviderAvailability")]
pub struct WireProviderAvailability {
    pub provider: WireAgentProvider,
    pub available: bool,
    /// A reason string when `available` is `false`; `null` when available.
    pub detail: Option<String>,
    pub capabilities: WireProviderCapabilities,
}

impl From<(ProviderAvailability, AgentCapabilities)> for WireProviderAvailability {
    fn from((availability, capabilities): (ProviderAvailability, AgentCapabilities)) -> Self {
        WireProviderAvailability {
            provider: availability.provider.into(),
            available: availability.available,
            detail: availability.detail,
            capabilities: capabilities.into(),
        }
    }
}

/// JSON shape of `GET /api/providers`: launch availability for every known
/// provider.
///
/// Wrapped in an object (rather than a bare array) so the response can grow
/// sibling fields later without breaking the contract, mirroring
/// [`WirePullRequestsResponse`](crate::rest::WirePullRequestsResponse).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "ProvidersResponse")]
pub struct WireProvidersResponse {
    pub providers: Vec<WireProviderAvailability>,
}

impl From<Vec<(ProviderAvailability, AgentCapabilities)>> for WireProvidersResponse {
    fn from(list: Vec<(ProviderAvailability, AgentCapabilities)>) -> Self {
        WireProvidersResponse {
            providers: list.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::AgentProvider;
    use delta_usecase::{
        ContextInjectionCapability, EventCapability, ForkCapability, InterruptCapability,
        LaunchCapability, PermissionCapability, ResumeCapability, SessionIdentityCapability,
        SteerCapability, TranscriptCapability,
    };

    /// A capability profile with the given terminal surface; the other fields
    /// are placeholders the wire projection does not read.
    fn caps_with_terminal(terminal: TerminalCapability) -> AgentCapabilities {
        AgentCapabilities {
            launch: LaunchCapability::PtyCommand,
            session_identity: SessionIdentityCapability::DeltaCanSetId,
            resume: ResumeCapability::Supported,
            events: EventCapability::HookAndTranscript,
            transcript: TranscriptCapability::JsonlFile,
            permission: PermissionCapability::HookDecision,
            context_injection: ContextInjectionCapability::HiddenPerTurn,
            interrupt: InterruptCapability::PaneKeystroke,
            terminal,
            fork: ForkCapability::None,
            steer: SteerCapability::None,
        }
    }

    #[test]
    fn available_terminal_provider_serializes_with_null_detail_and_has_terminal() {
        let response = WireProvidersResponse::from(vec![(
            ProviderAvailability {
                provider: AgentProvider::Claude,
                available: true,
                detail: None,
            },
            caps_with_terminal(TerminalCapability::AttachablePty),
        )]);
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({
                "providers": [
                    {
                        "provider": "claude",
                        "available": true,
                        "detail": null,
                        "capabilities": { "has_terminal": true }
                    }
                ]
            }),
        );
    }

    #[test]
    fn unavailable_provider_carries_its_reason_and_its_capabilities() {
        // Even an unavailable provider reports its (static) capability profile:
        // the profile does not depend on the binary being installed. Codex has
        // no terminal.
        let response = WireProvidersResponse::from(vec![(
            ProviderAvailability {
                provider: AgentProvider::Codex,
                available: false,
                detail: Some("The 'codex' binary for codex was not found on PATH.".to_owned()),
            },
            caps_with_terminal(TerminalCapability::NoTerminal),
        )]);
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["providers"][0]["provider"], "codex");
        assert_eq!(value["providers"][0]["available"], false);
        assert_eq!(
            value["providers"][0]["detail"],
            "The 'codex' binary for codex was not found on PATH."
        );
        assert_eq!(value["providers"][0]["capabilities"]["has_terminal"], false);
    }

    /// The `NoPtyNeeded` middle variant also projects to `has_terminal: false`:
    /// only an attachable PTY earns a terminal tab.
    #[test]
    fn no_pty_needed_projects_to_no_terminal() {
        let caps =
            WireProviderCapabilities::from(caps_with_terminal(TerminalCapability::NoPtyNeeded));
        assert!(!caps.has_terminal);
    }
}
