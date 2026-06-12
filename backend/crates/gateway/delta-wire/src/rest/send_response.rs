//! Response for `POST /api/sends`.

use delta_model::Send;
use serde::Serialize;
use ts_rs::TS;

use crate::send::WireSend;

/// Response for `POST /api/sends`: the queued send, including the thread it was
/// attributed to (a freshly created child thread for a branch send).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SendResponse")]
pub struct WireSendResponse {
    pub send: WireSend,
}

impl From<Send> for WireSendResponse {
    fn from(send: Send) -> Self {
        WireSendResponse { send: send.into() }
    }
}
