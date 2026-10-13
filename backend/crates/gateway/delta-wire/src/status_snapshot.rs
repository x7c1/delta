//! The wire form of a session's usage snapshot.

use delta_usecase::StatusSnapshot;
use serde::Serialize;
use ts_rs::TS;

use crate::agent_provider::WireAgentProvider;
use crate::rate_limit_window::WireRateLimitWindow;

/// The wire form of a session's usage snapshot. Every field is optional (a
/// session before its provider's first usage report carries `null`s), and a
/// provider that reports usage on several frames sends one snapshot per frame,
/// each stating only what that frame said; see `delta_usecase::StatusSnapshot`
/// for the semantics.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(rename = "StatusSnapshot")]
pub struct WireStatusSnapshot {
    /// Which provider's account and session these numbers describe. The client
    /// keys the account-scoped rate limits by this, so one provider's limits
    /// are never shown for another's session.
    pub provider: WireAgentProvider,
    pub model_id: Option<String>,
    pub model_display_name: Option<String>,
    pub context_used_percentage: Option<f64>,
    pub context_window_size: Option<u64>,
    pub context_current_usage: Option<u64>,
    pub total_input_tokens: Option<u64>,
    /// The account's rate-limit windows, most significant first. `null` means
    /// this snapshot says nothing about rate limits (so the client keeps what it
    /// has); `[]` means the account has none to show.
    pub rate_limits: Option<Vec<WireRateLimitWindow>>,
    pub total_cost_usd: Option<f64>,
    pub current_dir: Option<String>,
}

impl From<StatusSnapshot> for WireStatusSnapshot {
    fn from(snapshot: StatusSnapshot) -> Self {
        Self {
            provider: snapshot.provider.into(),
            model_id: snapshot.model_id,
            model_display_name: snapshot.model_display_name,
            context_used_percentage: snapshot.context_used_percentage,
            context_window_size: snapshot.context_window_size,
            context_current_usage: snapshot.context_current_usage,
            total_input_tokens: snapshot.total_input_tokens,
            rate_limits: snapshot
                .rate_limits
                .map(|windows| windows.into_iter().map(WireRateLimitWindow::from).collect()),
            total_cost_usd: snapshot.total_cost_usd,
            current_dir: snapshot.current_dir,
        }
    }
}
