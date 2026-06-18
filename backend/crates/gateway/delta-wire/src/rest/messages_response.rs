//! Response for `GET /api/threads/{id}/messages`.

use delta_model::Message;
use serde::Serialize;
use ts_rs::TS;

use crate::message::WireMessage;

/// Response for `GET /api/threads/{id}/messages`.
///
/// Holds `Vec<WireMessage>`, which carries an `f64` (`response_time_ms`), so
/// this derives only `PartialEq` — a float cannot implement `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
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
