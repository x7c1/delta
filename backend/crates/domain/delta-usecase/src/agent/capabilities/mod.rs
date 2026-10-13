//! [`AgentCapabilities`]: what a provider can and cannot do.
//!
//! The core switches UI and behaviour on these, never on
//! [`super::AgentProvider`] directly. A capability that a provider lacks is
//! made *explicit* here (rather than implied by a missing branch), so a new
//! provider declares its gaps and the UI degrades deliberately instead of
//! silently misbehaving.

mod context_injection_capability;
pub use context_injection_capability::ContextInjectionCapability;

mod event_capability;
pub use event_capability::EventCapability;

mod fork_capability;
pub use fork_capability::ForkCapability;

mod interrupt_capability;
pub use interrupt_capability::InterruptCapability;

mod launch_capability;
pub use launch_capability::LaunchCapability;

mod permission_capability;
pub use permission_capability::PermissionCapability;

mod resume_capability;
pub use resume_capability::ResumeCapability;

mod session_identity_capability;
pub use session_identity_capability::SessionIdentityCapability;

mod session_scoped_allow_capability;
pub use session_scoped_allow_capability::SessionScopedAllowCapability;

mod steer_capability;
pub use steer_capability::SteerCapability;

mod terminal_capability;
pub use terminal_capability::TerminalCapability;

mod transcript_capability;
pub use transcript_capability::TranscriptCapability;

/// The full capability profile a provider declares.
///
/// Every field is required: a provider must state each capability explicitly so
/// no gap is left implicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCapabilities {
    pub launch: LaunchCapability,
    pub session_identity: SessionIdentityCapability,
    pub resume: ResumeCapability,
    pub events: EventCapability,
    pub transcript: TranscriptCapability,
    pub permission: PermissionCapability,
    pub session_scoped_allow: SessionScopedAllowCapability,
    pub context_injection: ContextInjectionCapability,
    pub interrupt: InterruptCapability,
    pub terminal: TerminalCapability,
    /// Unused in v1 (always [`ForkCapability::None`]); carried for the future.
    pub fork: ForkCapability,
    /// Unused in v1 (always [`SteerCapability::None`]); carried for the future.
    pub steer: SteerCapability,
}

impl AgentCapabilities {
    /// Whether this provider's sessions are adapter-backed (terminal-less)
    /// rather than PTY-native. Forwards to
    /// [`LaunchCapability::is_adapter_backed`] — the launch capability is what
    /// determines which session paths (spawn, resume, dispatch) apply.
    pub fn is_adapter_backed(&self) -> bool {
        self.launch.is_adapter_backed()
    }

    /// Whether this provider understands
    /// [`PermissionDecision::AllowForSession`]. The single predicate both the
    /// wire projection (what the UI is told) and the decision path (what the
    /// server accepts) read, so the button the browser offers and the decision
    /// the server honours can never disagree.
    ///
    /// [`PermissionDecision::AllowForSession`]:
    ///     crate::interactor::PermissionDecision::AllowForSession
    pub fn supports_session_scoped_allow(&self) -> bool {
        matches!(
            self.session_scoped_allow,
            SessionScopedAllowCapability::Supported
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dispatch predicate: a PTY-launched provider (Claude) is native,
    /// every other launch capability is adapter-backed. This is the single bit
    /// the session paths key provider dispatch on, so pin both sides.
    #[test]
    fn pty_command_is_the_only_native_launch_capability() {
        assert!(!LaunchCapability::PtyCommand.is_adapter_backed());
        assert!(LaunchCapability::JsonRpcAppServer.is_adapter_backed());
    }

    /// The profile-level predicate forwards to the launch capability, ignoring
    /// every other field — two profiles differing only in `launch` disagree.
    #[test]
    fn profile_predicate_follows_the_launch_capability() {
        let base = AgentCapabilities {
            launch: LaunchCapability::PtyCommand,
            session_identity: SessionIdentityCapability::DeltaCanSetId,
            resume: ResumeCapability::Supported,
            events: EventCapability::HookAndTranscript,
            transcript: TranscriptCapability::JsonlFile,
            permission: PermissionCapability::HookDecision,
            session_scoped_allow: SessionScopedAllowCapability::Unsupported,
            context_injection: ContextInjectionCapability::HiddenPerTurn,
            interrupt: InterruptCapability::PaneKeystroke,
            terminal: TerminalCapability::AttachablePty,
            fork: ForkCapability::None,
            steer: SteerCapability::None,
        };
        assert!(!base.is_adapter_backed());
        let adapter_backed = AgentCapabilities {
            launch: LaunchCapability::JsonRpcAppServer,
            ..base
        };
        assert!(adapter_backed.is_adapter_backed());

        // The session-scoped allow is declared on its own field, independent of
        // how the provider is launched: an adapter-backed profile that has not
        // declared it still refuses the decision.
        assert!(!base.supports_session_scoped_allow());
        assert!(!adapter_backed.supports_session_scoped_allow());
        assert!(AgentCapabilities {
            session_scoped_allow: SessionScopedAllowCapability::Supported,
            ..base
        }
        .supports_session_scoped_allow());
    }
}
