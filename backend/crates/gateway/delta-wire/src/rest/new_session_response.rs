//! Response for `POST /api/sessions`.

use delta_usecase::SessionLifecycle;
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of the tmux/process lifecycle after an eager session spawn.
///
/// Mirrors the use-case [`SessionLifecycle`] variant-for-variant; see that
/// type for the semantics. This wire twin carries the serialization concerns
/// the domain must not know about: the snake_case variant names and the
/// TypeScript export. `ready` — the session already existed and was reused.
/// `starting` — the session was just created and may still be coming up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "SessionLifecycle")]
pub enum WireSessionLifecycle {
    Ready,
    Starting,
}

impl From<SessionLifecycle> for WireSessionLifecycle {
    fn from(lifecycle: SessionLifecycle) -> Self {
        match lifecycle {
            SessionLifecycle::Ready => WireSessionLifecycle::Ready,
            SessionLifecycle::Starting => WireSessionLifecycle::Starting,
        }
    }
}

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
