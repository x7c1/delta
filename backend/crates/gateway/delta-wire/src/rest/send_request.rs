//! Request body for `POST /api/sends`.

use delta_model::{MessageUuid, ThreadId};
use delta_usecase::SendTarget;
use serde::Deserialize;
use ts_rs::TS;

use super::worktree_spec::WireWorktreeSpec;

/// Request body for `POST /api/sends`.
///
/// A send either continues an existing session — by naming the `thread_id` it
/// is attributed to (the session is derived from that thread) — or starts a new
/// session, by setting `new_session: true` and omitting `thread_id`. The session
/// is determined entirely by the request; there is no implicit "current"
/// session.
///
/// This is the only REST body that flows inward, so it is the wire twin of the
/// domain [`SendTarget`]: it carries the deserialization concerns (field
/// defaults, plain `String`/`i64` ids) and resolves into the domain target via
/// [`into_target`](Self::into_target). All fields except `text` are optional in
/// the TypeScript export, matching what serde accepts.
#[derive(Debug, Deserialize, TS)]
#[ts(rename = "CreateSendRequest")]
pub struct WireCreateSendRequest {
    /// The thread to send into (typically a session's `main`). When
    /// `semantic_parent_uuid` is set this is the parent thread the new branch is
    /// created off. Omitted (with `new_session: true`) to start a fresh session.
    #[serde(default)]
    #[ts(optional)]
    pub thread_id: Option<i64>,
    /// Start a fresh session and land this message on its `main` thread. Mutually
    /// exclusive with `thread_id`.
    #[serde(default)]
    #[ts(optional, as = "Option<_>")]
    pub new_session: bool,
    /// When present, this is a branch send: the Interactor creates an unnamed
    /// child thread off this message and attributes the send to it. Only valid
    /// together with `thread_id`.
    #[serde(default)]
    #[ts(optional)]
    pub semantic_parent_uuid: Option<String>,
    /// The text to send into the session.
    pub text: String,
    /// An optional quote to inject as `additionalContext` on the matched turn.
    /// Ignored for a `new_session` send (a brand-new session has no earlier
    /// passage to anchor): it is echoed in the synthetic response but dropped
    /// before the first prompt, so the persisted row carries no quote.
    #[serde(default)]
    #[ts(optional)]
    pub locator_quote: Option<String>,
    /// The working directory a fresh session should start in. Only meaningful
    /// with `new_session: true`; for a thread send the session already has a
    /// fixed cwd, so this is ignored. When omitted, a `new_session` send uses
    /// the default per-spawn directory. The path is validated (it must be an
    /// existing directory) before the session launches.
    #[serde(default)]
    #[ts(optional)]
    pub workdir: Option<String>,
    /// The ids of registered launch options to apply to a fresh session's
    /// `claude` launch, in the order the user selected them. Only meaningful
    /// with `new_session: true`; for a thread send the session is already
    /// running, so this is ignored. When omitted (or empty) a session starts
    /// with no extra launch flags. Each id is resolved to its registered flag
    /// record at spawn and contributes argv entries.
    #[serde(default)]
    #[ts(optional)]
    pub launch_option_ids: Option<Vec<i64>>,
    /// An opt-in request to start a fresh session inside a git worktree of the
    /// selected `workdir`. Only meaningful with `new_session: true` and a
    /// `workdir` that is a git repository; for a thread send (or a new session
    /// without a workdir) it is rejected/ignored downstream. When omitted, the
    /// session starts directly in the selected/default directory (the unchanged
    /// behavior).
    #[serde(default)]
    #[ts(optional)]
    pub worktree: Option<WireWorktreeSpec>,
}

/// Why a [`WireCreateSendRequest`] could not be resolved to a [`SendTarget`].
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

impl WireCreateSendRequest {
    /// Resolve the request into a [`SendTarget`], or report a shape conflict.
    pub fn into_target(self) -> Result<(SendTarget, String, Option<String>), SendTargetError> {
        let target = match (self.thread_id, self.new_session) {
            (Some(_), true) => return Err(SendTargetError::Conflicting),
            (None, false) => return Err(SendTargetError::Unspecified),
            (Some(thread_id), false) => SendTarget::Thread {
                thread_id: ThreadId(thread_id),
                branch_from: self.semantic_parent_uuid.map(MessageUuid::from),
            },
            (None, true) => {
                if self.semantic_parent_uuid.is_some() {
                    return Err(SendTargetError::BranchOnNewSession);
                }
                SendTarget::NewSession {
                    workdir: self.workdir,
                    launch_option_ids: self.launch_option_ids.unwrap_or_default(),
                    worktree: self.worktree.map(Into::into),
                }
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
        let req = WireCreateSendRequest {
            thread_id: Some(7),
            new_session: false,
            semantic_parent_uuid: None,
            text: "hi".into(),
            locator_quote: Some("q".into()),
            workdir: None,
            launch_option_ids: None,
            worktree: None,
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
        let req = WireCreateSendRequest {
            thread_id: Some(3),
            new_session: false,
            semantic_parent_uuid: Some("uuid-parent".into()),
            text: "branch".into(),
            locator_quote: None,
            workdir: None,
            launch_option_ids: None,
            worktree: None,
        };
        let (target, _, _) = req.into_target().expect("a branch send is valid");
        match target {
            SendTarget::Thread {
                thread_id,
                branch_from,
            } => {
                assert_eq!(thread_id, ThreadId(3));
                assert_eq!(
                    branch_from,
                    Some(MessageUuid::from("uuid-parent")),
                    "the branch roots at the parent"
                );
            }
            SendTarget::NewSession { .. } => {
                panic!("a thread_id send must not be a NewSession target")
            }
        }
    }

    /// `new_session: true` with no thread resolves to a `NewSession` target.
    #[test]
    fn new_session_without_thread_resolves_to_a_new_session_target() {
        let req = WireCreateSendRequest {
            thread_id: None,
            new_session: true,
            semantic_parent_uuid: None,
            text: "kick off".into(),
            locator_quote: None,
            workdir: None,
            launch_option_ids: None,
            worktree: None,
        };
        let (target, _, _) = req.into_target().expect("a new-session send is valid");
        assert!(matches!(
            target,
            SendTarget::NewSession {
                workdir: None,
                launch_option_ids,
                ..
            } if launch_option_ids.is_empty()
        ));
    }

    /// A `new_session` send carrying a `workdir` maps that directory onto the
    /// `NewSession` target, where it is later validated before launch.
    #[test]
    fn new_session_with_workdir_carries_the_directory() {
        let req = WireCreateSendRequest {
            thread_id: None,
            new_session: true,
            semantic_parent_uuid: None,
            text: "in a project".into(),
            locator_quote: None,
            workdir: Some("/projects/app".into()),
            launch_option_ids: None,
            worktree: None,
        };
        let (target, _, _) = req.into_target().expect("a new-session send is valid");
        assert!(
            matches!(target, SendTarget::NewSession { workdir, .. } if workdir.as_deref() == Some("/projects/app")),
            "the workdir rides on the NewSession target"
        );
    }

    /// A `workdir` on a thread send is ignored: an existing thread's session
    /// already has a fixed cwd, so the request resolves to a plain `Thread`
    /// target with no working-directory override.
    #[test]
    fn workdir_is_ignored_for_a_thread_send() {
        let req = WireCreateSendRequest {
            thread_id: Some(5),
            new_session: false,
            semantic_parent_uuid: None,
            text: "hi".into(),
            locator_quote: None,
            workdir: Some("/ignored".into()),
            launch_option_ids: None,
            worktree: None,
        };
        let (target, _, _) = req.into_target().expect("a plain thread send is valid");
        assert!(matches!(
            target,
            SendTarget::Thread {
                thread_id: ThreadId(5),
                branch_from: None,
            }
        ));
    }

    /// Naming neither a thread nor a new session is the `Unspecified` conflict
    /// (the API maps it to 400).
    #[test]
    fn neither_thread_nor_new_session_is_unspecified() {
        let req = WireCreateSendRequest {
            thread_id: None,
            new_session: false,
            semantic_parent_uuid: None,
            text: "no target".into(),
            locator_quote: None,
            workdir: None,
            launch_option_ids: None,
            worktree: None,
        };
        assert_eq!(req.into_target().unwrap_err(), SendTargetError::Unspecified);
    }

    /// Naming BOTH a thread and a new session is the `Conflicting` case: the two
    /// targets are mutually exclusive, so this is a 400 rather than a silent
    /// pick-one.
    #[test]
    fn thread_and_new_session_together_is_conflicting() {
        let req = WireCreateSendRequest {
            thread_id: Some(1),
            new_session: true,
            semantic_parent_uuid: None,
            text: "both".into(),
            locator_quote: None,
            workdir: None,
            launch_option_ids: None,
            worktree: None,
        };
        assert_eq!(req.into_target().unwrap_err(), SendTargetError::Conflicting);
    }

    /// A branch (`semantic_parent_uuid`) on a `new_session` send is the
    /// `BranchOnNewSession` case: a brand-new session has no message to branch
    /// from, so this is a 400, not a dropped parent.
    #[test]
    fn branch_on_a_new_session_is_rejected() {
        let req = WireCreateSendRequest {
            thread_id: None,
            new_session: true,
            semantic_parent_uuid: Some("uuid-parent".into()),
            text: "branch on new".into(),
            locator_quote: None,
            workdir: None,
            launch_option_ids: None,
            worktree: None,
        };
        assert_eq!(
            req.into_target().unwrap_err(),
            SendTargetError::BranchOnNewSession
        );
    }

    /// The JSON the frontend sends today (omitted optional fields) still
    /// deserializes: omitted fields default rather than erroring.
    #[test]
    fn omitted_optional_fields_default_on_deserialize() {
        let req: WireCreateSendRequest =
            serde_json::from_str(r#"{"thread_id":7,"text":"hi"}"#).unwrap();
        assert_eq!(req.thread_id, Some(7));
        assert!(!req.new_session);
        assert_eq!(req.semantic_parent_uuid, None);
        assert_eq!(req.locator_quote, None);
        assert_eq!(req.workdir, None);
        assert_eq!(req.launch_option_ids, None);

        let req: WireCreateSendRequest =
            serde_json::from_str(r#"{"new_session":true,"text":"go","workdir":"/p"}"#).unwrap();
        assert_eq!(req.thread_id, None);
        assert!(req.new_session);
        assert_eq!(req.workdir.as_deref(), Some("/p"));
    }

    /// A `new_session` send carrying `launch_option_ids` maps them onto the
    /// `NewSession` target in the order given, where they are resolved to argv
    /// flags before launch.
    #[test]
    fn new_session_with_launch_option_ids_carries_them_in_order() {
        let req = WireCreateSendRequest {
            thread_id: None,
            new_session: true,
            semantic_parent_uuid: None,
            text: "with options".into(),
            locator_quote: None,
            workdir: None,
            launch_option_ids: Some(vec![3, 1, 2]),
            worktree: None,
        };
        let (target, _, _) = req.into_target().expect("a new-session send is valid");
        assert!(
            matches!(
                target,
                SendTarget::NewSession { ref launch_option_ids, .. }
                    if launch_option_ids == &[3, 1, 2]
            ),
            "the selected launch-option ids ride on the NewSession target in order"
        );
    }

    /// A `new_session` send carrying a `worktree` maps it onto the `NewSession`
    /// target, where it gates the worktree-at-launch path.
    #[test]
    fn new_session_with_worktree_carries_the_spec() {
        use delta_usecase::WorktreeStartPoint;

        let req: WireCreateSendRequest = serde_json::from_str(
            r#"{
                "new_session": true,
                "text": "in a worktree",
                "workdir": "/projects/app",
                "worktree": { "start_point": { "kind": "remote_branch", "name": "main" } }
            }"#,
        )
        .unwrap();
        let (target, _, _) = req.into_target().expect("a new-session send is valid");
        match target {
            SendTarget::NewSession { worktree, .. } => {
                let spec = worktree.expect("the worktree spec rides on the target");
                assert_eq!(
                    spec.start_point,
                    WorktreeStartPoint::RemoteBranch("main".to_owned()),
                );
            }
            SendTarget::Thread { .. } => panic!("a new_session send must not be a Thread target"),
        }
    }

    /// A `worktree` on a thread send is dropped: an existing thread's session is
    /// already running, so the request resolves to a plain `Thread` target.
    #[test]
    fn worktree_is_ignored_for_a_thread_send() {
        let req = WireCreateSendRequest {
            thread_id: Some(5),
            new_session: false,
            semantic_parent_uuid: None,
            text: "hi".into(),
            locator_quote: None,
            workdir: None,
            launch_option_ids: None,
            worktree: Some(WireWorktreeSpec {
                start_point: super::super::worktree_spec::WireWorktreeStartPoint::Head,
            }),
        };
        let (target, _, _) = req.into_target().expect("a plain thread send is valid");
        assert!(matches!(
            target,
            SendTarget::Thread {
                thread_id: ThreadId(5),
                branch_from: None,
            }
        ));
    }
}
