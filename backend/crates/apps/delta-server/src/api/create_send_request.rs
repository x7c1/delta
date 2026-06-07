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
    /// Ignored for a `new_session` send (a brand-new session has no earlier
    /// passage to anchor): it is echoed in the synthetic response but dropped
    /// before the first prompt, so the persisted row carries no quote.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain `thread_id` (no branch, no new session) resolves to a `Thread`
    /// target whose session is derived from that thread downstream.
    #[test]
    fn thread_id_resolves_to_a_plain_thread_target() {
        let req = CreateSendRequest {
            thread_id: Some(ThreadId(7)),
            new_session: false,
            semantic_parent_uuid: None,
            text: "hi".into(),
            locator_quote: Some("q".into()),
        };
        let (target, text, quote) = req.into_target().expect("a plain thread send is valid");
        assert!(
            matches!(
                target,
                SendTarget::Thread {
                    thread_id: ThreadId(7),
                    branch_from: None,
                }
            ),
            "a bare thread_id is a plain thread send"
        );
        assert_eq!(text, "hi");
        assert_eq!(quote.as_deref(), Some("q"));
    }

    /// A `thread_id` plus a `semantic_parent_uuid` resolves to a branch send:
    /// the same parent thread carrying the message to branch from.
    #[test]
    fn thread_id_with_semantic_parent_resolves_to_a_branch_target() {
        let parent = MessageUuid::from("uuid-parent");
        let req = CreateSendRequest {
            thread_id: Some(ThreadId(3)),
            new_session: false,
            semantic_parent_uuid: Some(parent.clone()),
            text: "branch".into(),
            locator_quote: None,
        };
        let (target, _, _) = req.into_target().expect("a branch send is valid");
        match target {
            SendTarget::Thread {
                thread_id,
                branch_from,
            } => {
                assert_eq!(thread_id, ThreadId(3));
                assert_eq!(branch_from, Some(parent), "the branch roots at the parent");
            }
            SendTarget::NewSession => panic!("a thread_id send must not be a NewSession target"),
        }
    }

    /// `new_session: true` with no thread resolves to a `NewSession` target.
    #[test]
    fn new_session_without_thread_resolves_to_a_new_session_target() {
        let req = CreateSendRequest {
            thread_id: None,
            new_session: true,
            semantic_parent_uuid: None,
            text: "kick off".into(),
            locator_quote: None,
        };
        let (target, _, _) = req.into_target().expect("a new-session send is valid");
        assert!(matches!(target, SendTarget::NewSession));
    }

    /// Naming neither a thread nor a new session is the `Unspecified` conflict
    /// (the API maps it to 400).
    #[test]
    fn neither_thread_nor_new_session_is_unspecified() {
        let req = CreateSendRequest {
            thread_id: None,
            new_session: false,
            semantic_parent_uuid: None,
            text: "no target".into(),
            locator_quote: None,
        };
        assert_eq!(req.into_target().unwrap_err(), SendTargetError::Unspecified);
    }

    /// Naming BOTH a thread and a new session is the `Conflicting` case: the two
    /// targets are mutually exclusive, so this is a 400 rather than a silent
    /// pick-one.
    #[test]
    fn thread_and_new_session_together_is_conflicting() {
        let req = CreateSendRequest {
            thread_id: Some(ThreadId(1)),
            new_session: true,
            semantic_parent_uuid: None,
            text: "both".into(),
            locator_quote: None,
        };
        assert_eq!(req.into_target().unwrap_err(), SendTargetError::Conflicting);
    }

    /// A branch (`semantic_parent_uuid`) on a `new_session` send is the
    /// `BranchOnNewSession` case: a brand-new session has no message to branch
    /// from, so this is a 400, not a dropped parent.
    #[test]
    fn branch_on_a_new_session_is_rejected() {
        let req = CreateSendRequest {
            thread_id: None,
            new_session: true,
            semantic_parent_uuid: Some(MessageUuid::from("uuid-parent")),
            text: "branch on new".into(),
            locator_quote: None,
        };
        assert_eq!(
            req.into_target().unwrap_err(),
            SendTargetError::BranchOnNewSession
        );
    }
}
