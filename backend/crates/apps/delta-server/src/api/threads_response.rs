//! Response for `GET /api/threads`.

use serde::Serialize;

use delta_usecase::Thread;

/// Response for `GET /api/threads`.
#[derive(Debug, Serialize)]
pub struct ThreadsResponse {
    pub threads: Vec<Thread>,
}
