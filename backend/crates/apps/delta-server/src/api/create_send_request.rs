//! Request body for `POST /api/sends`.

use serde::Deserialize;

use delta_usecase::{MessageUuid, ThreadId};

/// Request body for `POST /api/sends`.
#[derive(Debug, Deserialize)]
pub struct CreateSendRequest {
    /// The thread to send into (typically `main`). When `semantic_parent_uuid`
    /// is set this is the parent thread the new branch is created off.
    pub thread_id: ThreadId,
    /// When present, this is a branch send: the Interactor creates an unnamed
    /// child thread off this message and attributes the send to it.
    #[serde(default)]
    pub semantic_parent_uuid: Option<MessageUuid>,
    /// The text to send into the session.
    pub text: String,
    /// An optional quote to inject as `additionalContext` on the matched turn.
    #[serde(default)]
    pub locator_quote: Option<String>,
}
