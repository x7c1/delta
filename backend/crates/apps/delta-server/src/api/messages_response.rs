//! Response for `GET /api/threads/{id}/messages`.

use serde::Serialize;

use delta_usecase::Message;

/// Response for `GET /api/threads/{id}/messages`.
#[derive(Debug, Serialize)]
pub struct MessagesResponse {
    pub messages: Vec<Message>,
}
