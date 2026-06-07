//! Request body for `POST /api/sends`.

use serde::Deserialize;

use delta_usecase::{MessageUuid, SendTarget, ThreadId};

/// Request body for `POST /api/sends`.
///
/// A send either continues an existing session — by naming the `thread_id` it
/// is attributed to (the session is derived from that thread) — or starts a new
/// session, by setting `new_session: true` and omitting `thread_id`. The session
/// is determined entirely by the request; there is no implicit "current"
/// session.
#[derive(Debug, Deserialize)]
pub struct CreateSendRequest {
    /// The thread to send into (typically a session's `main`). When
    /// `semantic_parent_uuid` is set this is the parent thread the new branch is
    /// created off. Omitted (with `new_session: true`) to start a fresh session.
    #[serde(default)]
    pub thread_id: Option<ThreadId>,
    /// Start a fresh session and land this message on its `main` thread. Mutually
    /// exclusive with `thread_id`.
    #[serde(default)]
    pub new_session: bool,
    /// When present, this is a branch send: the Interactor creates an unnamed
    /// child thread off this message and attributes the send to it. Only valid
    /// together with `thread_id`.
    #[serde(default)]
    pub semantic_parent_uuid: Option<MessageUuid>,
    /// The text to send into the session.
    pub text: String,
    /// An optional quote to inject as `additionalContext` on the matched turn.
    #[serde(default)]
    pub locator_quote: Option<String>,
}

/// Why a [`CreateSendRequest`] could not be resolved to a [`SendTarget`].
///
/// These are request-shape conflicts the schema alone cannot express, so they
/// are surfaced as `400 Bad Request` rather than a use-case error.
#[derive(Debug, PartialEq, Eq)]
pub enum SendTargetError {
    /// Neither `thread_id` nor `new_session` was given.
    Unspecified,
    /// Both `thread_id` and `new_session: true` were given.
    Conflicting,
    /// `new_session: true` was combined with a branch (`semantic_parent_uuid`),
    /// which has no message to branch from yet.
    BranchOnNewSession,
}

impl SendTargetError {
    /// A human-readable reason, used as the `400` response body.
    pub fn message(&self) -> &'static str {
        match self {
            SendTargetError::Unspecified => {
                "a send must target a thread (`thread_id`) or start a new session (`new_session`)"
            }
            SendTargetError::Conflicting => "`thread_id` and `new_session` are mutually exclusive",
            SendTargetError::BranchOnNewSession => {
                "`new_session` cannot be combined with a branch (`semantic_parent_uuid`)"
            }
        }
    }
}

impl CreateSendRequest {
    /// Resolve the request into a [`SendTarget`], or report a shape conflict.
    pub fn into_target(self) -> Result<(SendTarget, String, Option<String>), SendTargetError> {
        let target = match (self.thread_id, self.new_session) {
            (Some(_), true) => return Err(SendTargetError::Conflicting),
            (None, false) => return Err(SendTargetError::Unspecified),
            (Some(thread_id), false) => SendTarget::Thread {
                thread_id,
                branch_from: self.semantic_parent_uuid,
            },
            (None, true) => {
                if self.semantic_parent_uuid.is_some() {
                    return Err(SendTargetError::BranchOnNewSession);
                }
                SendTarget::NewSession
            }
        };
        Ok((target, self.text, self.locator_quote))
    }
}
