//! `Stop` payload.

use serde::{Deserialize, Serialize};

/// `Stop` payload.
#[derive(Debug, Deserialize, Serialize)]
pub struct StopPayload {
    pub session_id: String,
    #[serde(default)]
    pub stop_reason: Option<String>,
}
