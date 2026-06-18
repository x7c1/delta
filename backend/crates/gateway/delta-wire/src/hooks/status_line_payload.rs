//! `statusLine` payload.
//!
//! Unlike the other entries in this module, `statusLine` is not a Claude Code
//! *hook*: it is the `statusLine.command` Delta injects into the session
//! settings (see `delta-bootstrap`'s `render_session_settings`). Claude Code
//! invokes that command on every status-line refresh and pipes this JSON to it
//! on stdin; the command `curl`s it back to the server. None of this data is in
//! the transcript JSONL, so it is the only way the server learns the session's
//! selected model, context-window usage, rate limits, and cost.
//!
//! Every field that can be absent is modeled as `Option`. Measured against
//! Claude Code v2.1.179, before the first API response of a session
//! `rate_limits` is entirely absent and `context_window.current_usage` /
//! `used_percentage` are `null`; `rate_limits` is also absent on accounts
//! without a Pro/Max subscription. `#[serde(default)]` on the whole struct's
//! optional fields makes those absences deserialize cleanly, and serde's
//! default of ignoring unknown fields keeps it forward-compatible as Claude
//! Code adds fields across versions (`fast_mode` is one example not in the
//! public schema).

use serde::{Deserialize, Serialize};

/// `statusLine` payload: a snapshot of session state Claude Code pipes to its
/// configured status-line command on every refresh.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLinePayload {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub model: Option<StatusLineModel>,
    #[serde(default)]
    pub context_window: Option<StatusLineContextWindow>,
    #[serde(default)]
    pub rate_limits: Option<StatusLineRateLimits>,
    #[serde(default)]
    pub cost: Option<StatusLineCost>,
    #[serde(default)]
    pub workspace: Option<StatusLineWorkspace>,
    /// Present in Claude Code v2.1.179 but absent from the public schema; an
    /// example of a field Delta tolerates without modeling it everywhere.
    #[serde(default)]
    pub fast_mode: Option<bool>,
}

/// The selected model.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineModel {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

/// Context-window usage. `current_usage` / `used_percentage` are `null` before
/// the session's first API response.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineContextWindow {
    /// Percentage of the context window in use, precomputed by Claude Code
    /// against `context_window_size`. Forwarded verbatim — Delta never
    /// recomputes it from token counts.
    #[serde(default)]
    pub used_percentage: Option<f64>,
    #[serde(default)]
    pub context_window_size: Option<u64>,
    #[serde(default)]
    pub current_usage: Option<u64>,
    #[serde(default)]
    pub total_input_tokens: Option<u64>,
}

/// Rate-limit windows. Absent entirely before the first API response and on
/// accounts without a Pro/Max subscription.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineRateLimits {
    #[serde(default)]
    pub five_hour: Option<StatusLineRateLimitWindow>,
    #[serde(default)]
    pub seven_day: Option<StatusLineRateLimitWindow>,
}

/// One rate-limit window.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineRateLimitWindow {
    #[serde(default)]
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds at which this window resets.
    #[serde(default)]
    pub resets_at: Option<i64>,
}

/// Session cost.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineCost {
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
}

/// Workspace location.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineWorkspace {
    #[serde(default)]
    pub current_dir: Option<String>,
}
