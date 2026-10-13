//! Response for `POST /api/sessions`.

use delta_usecase::SessionLifecycle;
use serde::Serialize;
use ts_rs::TS;

use crate::rest::session_lifecycle::WireSessionLifecycle;

/// Response for `POST /api/sessions` (eager spawn of a new session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "NewSessionResponse")]
pub struct WireNewSessionResponse {
    pub status: WireSessionLifecycle,
}

impl From<SessionLifecycle> for WireNewSessionResponse {
    fn from(lifecycle: SessionLifecycle) -> Self {
        WireNewSessionResponse {
            status: lifecycle.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(WireNewSessionResponse::from(SessionLifecycle::Starting)).unwrap(),
            serde_json::json!({ "status": "starting" }),
        );
        assert_eq!(
            serde_json::to_value(WireNewSessionResponse::from(SessionLifecycle::Ready)).unwrap(),
            serde_json::json!({ "status": "ready" }),
        );
    }
}
