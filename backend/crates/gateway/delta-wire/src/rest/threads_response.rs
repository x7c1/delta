//! Response for `GET /api/sessions/{id}/threads`.

use delta_model::Thread;
use serde::Serialize;
use ts_rs::TS;

use crate::thread::WireThread;

/// Response for `GET /api/sessions/{id}/threads`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "ThreadsResponse")]
pub struct WireThreadsResponse {
    pub threads: Vec<WireThread>,
}

impl From<Vec<Thread>> for WireThreadsResponse {
    fn from(threads: Vec<Thread>) -> Self {
        WireThreadsResponse {
            threads: threads.into_iter().map(WireThread::from).collect(),
        }
    }
}
