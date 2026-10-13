//! One rate-limit window of a session's usage state.

#[cfg(doc)]
use super::StatusSnapshot;

/// One rate-limit window from a [`StatusSnapshot`].
///
/// Windows are identified by their **duration**, not by a name. Claude reports
/// two fixed windows (5 hours and 7 days) and Codex reports anonymous
/// `primary` / `secondary` windows carrying an explicit `windowDurationMins`,
/// so mapping either provider's windows onto the other's names would be a
/// fiction. Carrying the duration as data lets the browser label and pace a
/// window it has never seen before.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RateLimitWindow {
    /// How long the window is, in seconds — its identity, and what the browser
    /// labels the row from (`5h`, `7d`). `None` when the provider reports a
    /// window without saying how long it is; the window is still shown (its
    /// percentage is real), just unlabelled.
    pub duration_seconds: Option<i64>,
    /// Percentage of the window consumed.
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds at which the window resets.
    pub resets_at: Option<i64>,
}
