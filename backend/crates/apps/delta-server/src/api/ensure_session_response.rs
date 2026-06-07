//! Response for `POST /api/sessions`.

use serde::Serialize;

use delta_usecase::SessionLifecycle;

/// The Claude Code session's lifecycle state, serialized for the browser.
///
/// `ready` — the session already existed and was reused. `starting` — the
/// session was just created and may still be coming up.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Ready,
    Starting,
}

impl From<SessionLifecycle> for SessionState {
    fn from(lifecycle: SessionLifecycle) -> Self {
        match lifecycle {
            SessionLifecycle::Ready => SessionState::Ready,
            SessionLifecycle::Starting => SessionState::Starting,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EnsureSessionResponse {
    pub status: SessionState,
}
