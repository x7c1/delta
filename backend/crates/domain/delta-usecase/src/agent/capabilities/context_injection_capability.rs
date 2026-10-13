//! How out-of-band context is injected for a turn without polluting the visible prompt.

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
