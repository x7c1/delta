//! Context-window usage in the status-line payload.

use serde::{Deserialize, Serialize};

use crate::hooks::status_line_current_usage::StatusLineCurrentUsage;

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
    /// Per-bucket token breakdown of the current context occupancy. Claude Code
    /// sends this as an object (not a scalar); `null` before the first API
    /// response.
    #[serde(default)]
    pub current_usage: Option<StatusLineCurrentUsage>,
    #[serde(default)]
    pub total_input_tokens: Option<u64>,
}
