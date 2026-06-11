//! Response for `GET /api/threads/{id}/messages`.

use delta_model::Message;
use serde::Serialize;
use ts_rs::TS;

use crate::message::WireMessage;

/// Response for `GET /api/threads/{id}/messages`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "MessagesResponse")]
pub struct WireMessagesResponse {
    pub messages: Vec<WireMessage>,
}

impl From<Vec<Message>> for WireMessagesResponse {
    fn from(messages: Vec<Message>) -> Self {
        WireMessagesResponse {
            messages: messages.into_iter().map(WireMessage::from).collect(),
        }
    }
}
