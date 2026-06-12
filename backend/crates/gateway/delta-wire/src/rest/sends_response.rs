//! Response for `GET /api/sessions/{id}/sends`.

use delta_model::Send;
use serde::Serialize;
use ts_rs::TS;

use crate::send::WireSend;

/// Response for `GET /api/sessions/{id}/sends`: the session's open
/// (non-terminal) sends — status `queued` or `dispatched` — oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SendsResponse")]
pub struct WireSendsResponse {
    pub sends: Vec<WireSend>,
}

impl From<Vec<Send>> for WireSendsResponse {
    fn from(sends: Vec<Send>) -> Self {
        WireSendsResponse {
            sends: sends.into_iter().map(WireSend::from).collect(),
        }
    }
}
