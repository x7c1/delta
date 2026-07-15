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
    pub context_injection: ContextInjectionCapability,
    pub interrupt: InterruptCapability,
    pub terminal: TerminalCapability,
    /// Unused in v1 (always [`ForkCapability::None`]); carried for the future.
    pub fork: ForkCapability,
    /// Unused in v1 (always [`SteerCapability::None`]); carried for the future.
    pub steer: SteerCapability,
}
