//! The wire form of one rate-limit window.

use delta_usecase::RateLimitWindow;
use serde::Serialize;
use ts_rs::TS;

/// The wire form of one rate-limit window, identified by its duration rather
/// than by a provider-specific name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(rename = "RateLimitWindow")]
pub struct WireRateLimitWindow {
    /// How long the window is, in seconds — what the client labels the row
    /// from. `null` when the provider reported a window without its length.
    pub duration_seconds: Option<i64>,
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds at which the window resets.
    pub resets_at: Option<i64>,
}

impl From<RateLimitWindow> for WireRateLimitWindow {
    fn from(window: RateLimitWindow) -> Self {
        Self {
            duration_seconds: window.duration_seconds,
            used_percentage: window.used_percentage,
            resets_at: window.resets_at,
        }
    }
}
