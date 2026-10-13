//! One rate-limit window.

use serde::{Deserialize, Serialize};

/// One rate-limit window.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineRateLimitWindow {
    #[serde(default)]
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds at which this window resets.
    #[serde(default)]
    pub resets_at: Option<i64>,
}
