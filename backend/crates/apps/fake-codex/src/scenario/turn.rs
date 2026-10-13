//! What a `turn/start` request plays.

use serde::Deserialize;

use super::Emit;

/// What a `turn/start` request plays.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Turn {
    /// The id returned from `turn/start` (under `result.turn.id`) and wrapped
    /// into every emitted `turn/started` / `turn/completed` `params.turn`.
    #[serde(default = "super::default_turn_id")]
    pub turn_id: String,
    /// The notifications (and optional approval request) to emit, in order.
    #[serde(default)]
    pub emit: Vec<Emit>,
}
