//! The wire form of the provider-availability listing (`GET /api/providers`).

use delta_model::ProviderAvailability;
use delta_usecase::AgentCapabilities;
use serde::Serialize;
use ts_rs::TS;

use crate::rest::provider_availability::WireProviderAvailability;

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

    use crate::rest::launch_option_style::WireLaunchOptionStyle;
    use crate::rest::provider_capabilities::WireProviderCapabilities;

    use delta_model::AgentProvider;
    use delta_usecase::{
        ContextInjectionCapability, EventCapability, InterruptCapability, LaunchCapability,
        PermissionCapability, ResumeCapability, SessionIdentityCapability,
        SessionScopedAllowCapability, TerminalCapability, TranscriptCapability,
    };

    /// A capability profile with the given terminal surface and launch
    /// capability; the other fields are placeholders the wire projection does
    /// not read.
    fn caps(terminal: TerminalCapability, launch: LaunchCapability) -> AgentCapabilities {
        AgentCapabilities {
            launch,
            session_identity: SessionIdentityCapability::DeltaCanSetId,
            resume: ResumeCapability::Supported,
            events: EventCapability::HookAndTranscript,
            transcript: TranscriptCapability::JsonlFile,
            permission: PermissionCapability::HookDecision,
            session_scoped_allow: SessionScopedAllowCapability::Unsupported,
            context_injection: ContextInjectionCapability::HiddenPerTurn,
            interrupt: InterruptCapability::PaneKeystroke,
            terminal,
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
            caps(
                TerminalCapability::AttachablePty,
                LaunchCapability::PtyCommand,
            ),
        )]);
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({
                "providers": [
                    {
                        "provider": "claude",
                        "available": true,
                        "detail": null,
                        "capabilities": {
                            "has_terminal": true,
                            "has_comms_log": false,
                            "has_allow_for_session": false,
                            "launch_option_style": "cli_flag"
                        }
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
            caps(
                TerminalCapability::NoTerminal,
                LaunchCapability::JsonRpcAppServer,
            ),
        )]);
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["providers"][0]["provider"], "codex");
        assert_eq!(value["providers"][0]["available"], false);
        assert_eq!(
            value["providers"][0]["detail"],
            "The 'codex' binary for codex was not found on PATH."
        );
        assert_eq!(value["providers"][0]["capabilities"]["has_terminal"], false);
        assert_eq!(value["providers"][0]["capabilities"]["has_comms_log"], true);
        assert_eq!(
            value["providers"][0]["capabilities"]["launch_option_style"],
            "request_field"
        );
    }

    /// The comms-log capability follows the launch capability, not the terminal
    /// one — the two are complementary windows onto the same session, so a
    /// structured-launch provider earns the inspector and a command-launch one
    /// does not, whatever terminal surface either reports.
    #[test]
    fn the_comms_log_capability_follows_the_launch_capability() {
        let pty = WireProviderCapabilities::from(caps(
            TerminalCapability::AttachablePty,
            LaunchCapability::PtyCommand,
        ));
        assert!(
            !pty.has_comms_log,
            "a terminal program's window is its terminal, not a frame log"
        );

        let app_server = WireProviderCapabilities::from(caps(
            TerminalCapability::NoTerminal,
            LaunchCapability::JsonRpcAppServer,
        ));
        assert!(app_server.has_comms_log);
        assert!(
            !app_server.has_terminal,
            "the headless provider's only window is the frame log"
        );
    }

    /// The `NoPtyNeeded` middle variant also projects to `has_terminal: false`:
    /// only an attachable PTY earns a terminal tab.
    #[test]
    fn no_pty_needed_projects_to_no_terminal() {
        let projected = WireProviderCapabilities::from(caps(
            TerminalCapability::NoPtyNeeded,
            LaunchCapability::PtyCommand,
        ));
        assert!(!projected.has_terminal);
    }

    /// The session-scoped allow is its own declared capability, not something
    /// derived from how the provider is launched or which surface it offers: a
    /// profile that does not declare it projects to `false` however it is
    /// launched, and declaring it is the only thing that flips the flag.
    #[test]
    fn the_session_scoped_allow_flag_follows_its_own_capability() {
        let undeclared = caps(
            TerminalCapability::NoTerminal,
            LaunchCapability::JsonRpcAppServer,
        );
        assert!(
            !WireProviderCapabilities::from(undeclared).has_allow_for_session,
            "an adapter-backed provider does not get the capability for free"
        );

        let declared = AgentCapabilities {
            session_scoped_allow: SessionScopedAllowCapability::Supported,
            ..undeclared
        };
        assert!(WireProviderCapabilities::from(declared).has_allow_for_session);

        // And it is independent of the terminal surface too: a PTY provider that
        // declared it would report it.
        let pty_declared = AgentCapabilities {
            session_scoped_allow: SessionScopedAllowCapability::Supported,
            ..caps(
                TerminalCapability::AttachablePty,
                LaunchCapability::PtyCommand,
            )
        };
        assert!(WireProviderCapabilities::from(pty_declared).has_allow_for_session);
    }

    /// The launch-option style follows the launch capability, not the terminal
    /// one: a command-line launch means argv flags, a structured launch means
    /// request fields. Pin both directions so a provider cannot quietly switch
    /// the vocabulary Settings tells the user to write in.
    #[test]
    fn the_launch_option_style_follows_the_launch_capability() {
        let flag = WireProviderCapabilities::from(caps(
            TerminalCapability::AttachablePty,
            LaunchCapability::PtyCommand,
        ));
        assert_eq!(flag.launch_option_style, WireLaunchOptionStyle::CliFlag);

        // Same terminal surface, different launch: only the launch capability
        // moves the style.
        let field = WireProviderCapabilities::from(caps(
            TerminalCapability::AttachablePty,
            LaunchCapability::JsonRpcAppServer,
        ));
        assert_eq!(
            field.launch_option_style,
            WireLaunchOptionStyle::RequestField
        );
    }
}
