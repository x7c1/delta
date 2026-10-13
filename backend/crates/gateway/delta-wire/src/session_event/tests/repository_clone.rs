//! Repository clones, which belong to no session.

use super::*;

#[test]
fn a_repository_clone_event_serializes_without_a_session_id() {
    // The clone events are the exception to the "every frame names its
    // session" rule, so pin the absence: a client narrowing the union on
    // `session_id` must not find one here by accident.
    let value = json(&WireSessionEvent::from(
        SessionEvent::RepositoryCloneCompleted {
            repo_owner: "x7c1".into(),
            repo_name: "delta".into(),
            clone_root: "/home/dev/projects".into(),
            destination_path: "/home/dev/projects/delta".into(),
        },
    ));
    assert_eq!(
        value,
        serde_json::json!({
            "kind": "repository_clone_completed",
            "repo_owner": "x7c1",
            "repo_name": "delta",
            "clone_root": "/home/dev/projects",
            "destination_path": "/home/dev/projects/delta",
        }),
    );
}

#[test]
fn a_failed_repository_clone_carries_the_reason() {
    assert_eq!(
        json(&WireSessionEvent::from(
            SessionEvent::RepositoryCloneFailed {
                repo_owner: "x7c1".into(),
                repo_name: "delta".into(),
                clone_root: "/home/dev/projects".into(),
                destination_path: "/home/dev/projects/delta".into(),
                message: "repository not found".into(),
            }
        )),
        serde_json::json!({
            "kind": "repository_clone_failed",
            "repo_owner": "x7c1",
            "repo_name": "delta",
            "clone_root": "/home/dev/projects",
            "destination_path": "/home/dev/projects/delta",
            "message": "repository not found",
        }),
    );
}
