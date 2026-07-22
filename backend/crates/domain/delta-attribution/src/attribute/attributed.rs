//! [`Attributed`]: the outcome of folding one batch of transcript lines.

use delta_model::Message;

use super::{AttributionState, Effect};

/// The outcome of folding one batch of transcript lines.
///
/// Holds `Vec<Message>`, which carries an `f64` (`response_time_ms`), so this
/// derives only `PartialEq` — a float cannot implement `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub struct Attributed {
    /// The lines as attributed [`Message`]s, in input order.
    pub messages: Vec<Message>,
    /// The actions the caller must execute, in decision order.
    pub effects: Vec<Effect>,
    /// The state after the batch — the exact seed for folding the lines that
    /// follow.
    pub state: AttributionState,
}
