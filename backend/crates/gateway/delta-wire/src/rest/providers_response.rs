//! The wire form of the provider-availability listing (`GET /api/providers`).

use delta_model::ProviderAvailability;
#[cfg(doc)]
use delta_usecase::SessionScopedAllowCapability;
use delta_usecase::{AgentCapabilities, LaunchCapability, TerminalCapability};
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
    /// Whether the browser can inspect the frames Delta exchanges with this
    /// provider — i.e. whether this provider's sessions get the comms-log pane
    /// (the `/comms` stream) as their right-pane window.
    ///
    /// Derived from the internal [`LaunchCapability`], the same field
    /// [`Self::launch_option_style`] follows, because *how Delta drives a
    /// provider* is what decides whether there are frames to show:
    /// [`LaunchCapability::JsonRpcAppServer`] means Delta itself writes and reads
    /// every message, so the exchange is inspectable by construction → `true`;
    /// [`LaunchCapability::PtyCommand`] means Delta launched a terminal program
    /// and holds no message-level view of it — its window is the terminal
    /// instead → `false`.
    ///
    /// So the two flags are complementary, not independent: the right pane is the
    /// terminal when [`Self::has_terminal`], and the comms log when this is set.
    pub has_comms_log: bool,
    /// Whether this provider understands a permission decision scoped to the
    /// whole session (`allow_for_session`), rather than only to the one request
    /// being answered. Derived from the internal
    /// [`SessionScopedAllowCapability`]. The permission notice offers its
    /// session-scoped button only where this is `true` — a button that would
    /// earn a `400 permission_decision_unsupported` when pressed is worse than
    /// no button, so an unknown capability hides it.
    pub has_allow_for_session: bool,
    /// How this provider reads a registered launch option's `(name, value?)`
    /// pair. Settings words its launch-option form from this, so a user
    /// registering an option for a field-style provider is told to write
    /// `model`, not `--model`.
    pub launch_option_style: WireLaunchOptionStyle,
}

/// How a provider interprets a launch option's `name` and `value`.
///
/// A registered launch option is a provider-neutral `(name, value?)` pair; what
/// the pair *means* depends on how the provider is launched, which is why this
/// is a capability rather than something the UI derives from the provider name.
/// Delta validates neither names nor values (the agent that receives them owns
/// that vocabulary), so telling the user which vocabulary to write in is the
/// only guard-rail there is — hence carrying it on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "LaunchOptionStyle")]
pub enum WireLaunchOptionStyle {
    /// `name` is a command-line flag and `value` its argument
    /// (`--permission-mode auto`). A valueless option is a bare flag.
    CliFlag,
    /// `name` is a field of the provider's session-start request and `value`
    /// that field's value (Codex's `thread/start`: `model` → `gpt-5.6-sol`). A
    /// valueless option sets a bare boolean field.
    RequestField,
}

impl From<AgentCapabilities> for WireProviderCapabilities {
    fn from(capabilities: AgentCapabilities) -> Self {
        WireProviderCapabilities {
            has_terminal: matches!(capabilities.terminal, TerminalCapability::AttachablePty),
            has_comms_log: matches!(capabilities.launch, LaunchCapability::JsonRpcAppServer),
            has_allow_for_session: capabilities.supports_session_scoped_allow(),
            launch_option_style: capabilities.launch.into(),
        }
    }
}

impl From<LaunchCapability> for WireLaunchOptionStyle {
    /// Derived from *how the provider is launched*, the same profile field that
    /// decides which session path it takes: a provider Delta launches as a
    /// command line takes its options as argv flags, while one Delta drives over
    /// a structured request takes them as fields of that request. Deriving it
    /// keeps the adapter's capability profile the single source of truth, so a
    /// new provider cannot ship a launch style the UI has never heard of.
    fn from(launch: LaunchCapability) -> Self {
        match launch {
            LaunchCapability::PtyCommand => WireLaunchOptionStyle::CliFlag,
            LaunchCapability::JsonRpcAppServer => WireLaunchOptionStyle::RequestField,
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
        SessionScopedAllowCapability, SteerCapability, TranscriptCapability,
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
