//! The per-bucket token breakdown inside `context_window.current_usage`.

use serde::{Deserialize, Serialize};

/// The per-bucket token breakdown inside `context_window.current_usage`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineCurrentUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
}
