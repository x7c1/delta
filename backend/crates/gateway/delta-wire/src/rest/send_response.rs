//! Response for `POST /api/sends`.

use delta_model::PendingSend;
use serde::Serialize;
use ts_rs::TS;

use crate::pending_send::WirePendingSend;

/// Response for `POST /api/sends`: the queued send, including the thread it was
/// attributed to (a freshly created child thread for a branch send).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SendResponse")]
pub struct WireSendResponse {
    pub send: WirePendingSend,
}

impl From<PendingSend> for WireSendResponse {
    fn from(send: PendingSend) -> Self {
        WireSendResponse { send: send.into() }
    }
}
