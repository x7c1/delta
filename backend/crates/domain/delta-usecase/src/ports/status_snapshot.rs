//! A snapshot of a session's usage state.

use delta_model::AgentProvider;

use super::RateLimitWindow;

/// A snapshot of a session's usage state, as its provider's own edge reported
/// it.
///
/// Every field is optional, because every provider reports a different subset
/// at a different time: before a Claude session's first API response
/// `current_usage` / `used_percentage` are `null`, while Codex reports token
/// usage and account rate limits on two independent notifications, so a given
/// snapshot may speak to only one of them. See [`Self::rate_limits`] for how
/// "this frame says nothing about rate limits" (`None`) is distinguished from
/// "the account has none" (`Some(vec![])`, which is what a non-Pro/Max Claude
/// account's status line reports — not an absent field).
///
/// **The neutral layer never computes any of these numbers.** Each provider's
/// adapter (or hook) is the authority for its own: Claude Code precomputes
/// `used_percentage` against the correct window size and Delta forwards it
/// verbatim; the Codex adapter derives its percentage from the counts and the
/// `modelContextWindow` the server reports, and omits it when the server
/// reports no window. Recomputing here would mean guessing a window size the
/// core does not know.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSnapshot {
    /// Which provider's account and session these numbers describe.
    ///
    /// Load-bearing rather than decorative: rate limits are scoped to an
    /// account × provider, so the browser keys them by this and can never show
    /// one provider's limits while another provider's session is focused.
    pub provider: AgentProvider,
    /// The selected model's stable id (e.g. `claude-opus-4-...`).
    pub model_id: Option<String>,
    /// The selected model's human-readable name (e.g. `Opus 4.8`).
    pub model_display_name: Option<String>,
    /// Percentage of the context window in use, as computed by the provider's
    /// own edge — never recomputed here. `None` when the provider does not
    /// expose enough to say (e.g. Codex before the server reports a
    /// `modelContextWindow`), which reads as "no bar", never as 0%.
    pub context_used_percentage: Option<f64>,
    /// The context window's total size in tokens.
    pub context_window_size: Option<u64>,
    /// Tokens currently occupying the context window.
    pub context_current_usage: Option<u64>,
    /// Total input tokens sent this session.
    pub total_input_tokens: Option<u64>,
    /// The account's rate-limit windows, most significant first.
    ///
    /// `None` means this snapshot makes **no statement** about rate limits (a
    /// Codex token-usage frame says nothing about them, and must not clear
    /// what an earlier account frame reported). `Some(windows)` replaces the
    /// account's windows wholesale — including `Some(vec![])`, which is how a
    /// provider says "this account has no windows to show".
    pub rate_limits: Option<Vec<RateLimitWindow>>,
    /// Total session cost in USD.
    pub total_cost_usd: Option<f64>,
    /// The session's working directory.
    pub current_dir: Option<String>,
}

impl StatusSnapshot {
    /// An empty snapshot for `provider`: it states nothing at all, so a caller
    /// fills in only the facts its frame actually carried. There is no
    /// `Default`, because a snapshot with no provider would be a snapshot whose
    /// rate limits belong to nobody.
    pub fn new(provider: AgentProvider) -> Self {
        Self {
            provider,
            model_id: None,
            model_display_name: None,
            context_used_percentage: None,
            context_window_size: None,
            context_current_usage: None,
            total_input_tokens: None,
            rate_limits: None,
            total_cost_usd: None,
            current_dir: None,
        }
    }
}
