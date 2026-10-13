//! When the launch fires `SessionStart`.

use serde::Deserialize;

/// When the launch fires `SessionStart`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum SessionStartMode {
    /// `"immediate"` or `"skip"`.
    Named(String),
    /// `{ "delay_ms": N }`: fire after a scripted delay.
    Delayed { delay_ms: u64 },
}

impl Default for SessionStartMode {
    fn default() -> Self {
        Self::Named("immediate".to_owned())
    }
}
