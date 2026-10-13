//! The selected model.

use serde::{Deserialize, Serialize};

/// The selected model.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct StatusLineModel {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}
