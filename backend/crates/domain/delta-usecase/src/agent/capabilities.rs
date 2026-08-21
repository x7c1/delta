//! [`AgentCapabilities`]: what a provider can and cannot do.
//!
//! The core switches UI and behaviour on these, never on
//! [`super::AgentProvider`] directly. A capability that a provider lacks is
//! made *explicit* here (rather than implied by a missing branch), so a new
//! provider declares its gaps and the UI degrades deliberately instead of
//! silently misbehaving.

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
    /// [`AgentAdapter`]: super::AgentAdapter
    pub fn is_adapter_backed(self) -> bool {
        !matches!(self, LaunchCapability::PtyCommand)
    }
}

/// Who assigns the provider's session identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdentityCapability {
    /// Delta mints the id and pins it at launch (Claude's `--session-id`).
    DeltaCanSetId,
    /// The provider assigns the id and returns it (Codex's `thr_...`).
    ProviderReturnsId,
}

/// Whether a closed session can be reopened with its history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeCapability {
    /// Resume is supported.
    Supported,
    /// Resume is not supported.
    Unsupported,
}

/// How the provider delivers its event stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCapability {
    /// Structured, id-tagged turn/item events pushed over the wire (Codex).
    StructuredTurnEvents,
    /// Reconstructed from HTTP hooks plus a JSONL transcript tail (Claude).
    HookAndTranscript,
}

/// Whether, and how, a durable transcript is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptCapability {
    /// A JSONL transcript file the adapter tails (Claude).
    JsonlFile,
    /// The transcript is only what the pushed event stream carries.
    EventStreamOnly,
    /// No transcript is available.
    None,
}

/// Where a permission decision is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionCapability {
    /// The adapter answers the request directly (a structured RPC reply).
    AdapterDecision,
    /// The decision rides a hook response back to the provider (Claude).
    HookDecision,
    /// The provider owns its own permission UI; Delta cannot answer.
    ProviderUiOnly,
}

/// Whether the provider understands an allow that stands for the rest of the
/// session, rather than only for the one request being answered.
///
/// Deliberately *not* folded into [`PermissionCapability`]: that one answers
/// **where** a decision is made (adapter reply, hook response, or nowhere Delta
/// can reach), which is a different question from **which decisions exist**. A
/// provider could answer over an adapter and still know only per-request
/// decisions, so the two vary independently and are declared independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionScopedAllowCapability {
    /// The provider accepts an allow scoped to the whole session, and stops
    /// asking for comparable requests afterwards (Codex's `acceptForSession`).
    /// The scope lives entirely in the provider's session — Delta neither
    /// records which grants are outstanding nor replays them.
    Supported,
    /// Every decision answers exactly one request (Claude's permission hook,
    /// whose response `behavior` has no session-scoped form). Delta rejects a
    /// session-scoped decision for such a provider rather than degrading it
    /// into a plain allow, which would keep prompting a user who asked not to
    /// be prompted, with nothing said about why.
    Unsupported,
}

/// How out-of-band context is injected for a turn without polluting the visible
/// prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextInjectionCapability {
    /// Hidden context added per turn, invisible in the prompt (Claude's
    /// `UserPromptSubmit` hook `additionalContext`; Codex's
    /// `thread/inject_items`).
    HiddenPerTurn,
    /// Context supplied through a provider hook mechanism.
    HookContext,
    /// Context can only be prepended to the visible prompt text (a degraded
    /// fallback for providers with no hidden channel).
    VisiblePromptPrefix,
    /// No context injection is possible.
    None,
}

/// Whether an in-flight turn can be interrupted, and by what means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptCapability {
    /// Interrupt by injecting the interrupt keystroke into the PTY (Claude's
    /// `Escape`).
    PaneKeystroke,
    /// Interrupt via an explicit RPC (Codex's `turn/interrupt`).
    Rpc,
    /// Interrupt is not supported.
    Unsupported,
}

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

/// Whether the provider can fork a conversation into an independent branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkCapability {
    /// No native fork. All providers are `None` in v1.
    None,
    /// The provider forks natively into a new thread (reserved for future use).
    NativeThreadFork,
}

/// Whether the provider accepts steering input mid-turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerCapability {
    /// No mid-turn steering. All providers are `None` in v1.
    None,
    /// The provider accepts steering while a turn is in flight (reserved).
    MidTurn,
}

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
