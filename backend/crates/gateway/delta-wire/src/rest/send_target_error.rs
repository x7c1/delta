//! Why a create-send request could not be resolved to a send target.

/// Why a [`WireCreateSendRequest`](super::WireCreateSendRequest) could not be resolved to a
/// [`SendTarget`](delta_usecase::SendTarget).
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
    /// `pull_request_number` was given as zero or a negative number. Pull
    /// requests are numbered from 1, so nothing could ever resolve it.
    NonPositivePullRequestNumber,
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
            SendTargetError::NonPositivePullRequestNumber => {
                "`pull_request_number` must be a positive pull request number"
            }
        }
    }
}
