//! Workspace location.

use serde::{Deserialize, Serialize};

/// Workspace location.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineWorkspace {
    #[serde(default)]
    pub current_dir: Option<String>,
}
