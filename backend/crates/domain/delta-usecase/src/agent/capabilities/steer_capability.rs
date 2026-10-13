//! Whether the provider accepts steering input mid-turn.

/// Whether the provider accepts steering input mid-turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerCapability {
    /// No mid-turn steering. All providers are `None` in v1.
    None,
    /// The provider accepts steering while a turn is in flight (reserved).
    MidTurn,
}
