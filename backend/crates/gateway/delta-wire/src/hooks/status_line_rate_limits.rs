//! Rate-limit windows in the status-line payload.

use serde::{Deserialize, Serialize};

use crate::hooks::status_line_rate_limit_window::StatusLineRateLimitWindow;

/// Rate-limit windows. Absent entirely before the first API response and on
/// accounts without a Pro/Max subscription.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineRateLimits {
    #[serde(default)]
    pub five_hour: Option<StatusLineRateLimitWindow>,
    #[serde(default)]
    pub seven_day: Option<StatusLineRateLimitWindow>,
}
