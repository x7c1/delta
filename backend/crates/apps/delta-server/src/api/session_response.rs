//! Response for `GET /api/session`.

use serde::Serialize;

use delta_usecase::{Session, ThreadId};

/// Response for `GET /api/session`.
#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub session: Session,
    pub main_thread_id: ThreadId,
}
