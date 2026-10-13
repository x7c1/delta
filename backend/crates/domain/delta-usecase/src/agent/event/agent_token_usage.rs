//! Token accounting for one session, as the provider's own edge reported it.

/// Token accounting for one session, as the provider's own edge reported it.
///
/// Observability only — nothing here is persisted or fed to the turn machine.
/// The fields mirror [`crate::ports::StatusSnapshot`]'s context fields so the
/// pump can forward them without inventing anything.
///
/// **The percentage is the provider's to compute, never the core's** — only the
/// provider knows what window the counts are against (see
/// [`crate::ports::StatusSnapshot`] for what each edge does with that rule).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentTokenUsage {
    /// Percentage of the context window in use, or `None` when the provider
    /// cannot say (the window size is unknown). Never a placeholder — an absent
    /// percentage hides the bar rather than pinning it at 0%.
    pub context_used_percentage: Option<f64>,
    /// The context window's total size in tokens.
    pub context_window_size: Option<u64>,
    /// Tokens currently occupying the context window.
    pub context_current_usage: Option<u64>,
    /// Total input tokens sent this session.
    pub total_input_tokens: Option<u64>,
}
