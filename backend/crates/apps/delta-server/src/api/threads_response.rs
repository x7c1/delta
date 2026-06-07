//! Response for `GET /api/sessions/{id}/threads`.

use serde::Serialize;

use delta_usecase::Thread;

/// Response for `GET /api/sessions/{id}/threads`.
#[derive(Debug, Serialize)]
pub struct ThreadsResponse {
    pub threads: Vec<Thread>,
}
