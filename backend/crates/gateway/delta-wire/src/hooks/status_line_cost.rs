//! Session cost.

use serde::{Deserialize, Serialize};

/// Session cost.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineCost {
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
}
