//! Response for `POST /api/sends`.

use serde::Serialize;

use delta_usecase::PendingSend;

/// Response for `POST /api/sends`: the queued send, including the thread it was
/// attributed to (a freshly created child thread for a branch send).
#[derive(Debug, Serialize)]
pub struct CreateSendResponse {
    pub send: PendingSend,
}
