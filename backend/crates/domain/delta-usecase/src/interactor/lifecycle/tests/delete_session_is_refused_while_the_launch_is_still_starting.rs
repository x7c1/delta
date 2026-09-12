use crate::error::Error;
use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A session that has not finished starting cannot be removed, in either of the
/// two runtime sub-states the starting window has — and the refusal leaves the
/// launch intact, so it can still bind (or be cancelled by a close).
///
/// Refused with `SessionSpawning` rather than `SessionOpen`: neither sub-state
/// is *open* (nothing is bound to the session yet), and the two refusals ask the
/// user for different things — wait for it to come up, versus close it first.
#[tokio::test]
async fn delete_session_is_refused_while_the_launch_is_still_starting() {
    // --- Sub-state 1: launching (the preparation is still running) ---------
    let gate = WorktreeGate::closed();
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git")
        .with_gate(&gate);
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: Some("/projects/app".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: Some(WorktreeSpec {
                    start_point: WorktreeStartPoint::Head,
                }),
            },
            "start something",
            None,
        )
        .await
        .expect("the send is accepted while the worktree build is still held");
    let launching_id = send.session_id.clone();
    assert_eq!(
        ix.launching_session_ids().await,
        vec![launching_id.clone()],
        "the launch is parked on the gate, with no pane yet"
    );

    let err = ix
        .delete_session(&launching_id)
        .await
        .expect_err("a launch still preparing must not be removed");
    assert!(
        matches!(&err, Error::SessionSpawning(id) if id == launching_id.as_str()),
        "a still-preparing launch is surfaced as SessionSpawning, got {err:?}"
    );
    assert!(
        ix.store().session(&launching_id).await.unwrap().is_some(),
        "a refused removal deletes nothing"
    );
    assert_eq!(
        ix.launching_session_ids().await,
        vec![launching_id.clone()],
        "the refusal is non-destructive: the launching entry is still there to bind or be cancelled"
    );

    // --- Sub-state 2: pending (the pane is up, awaiting its first hook) ----
    let ix = interactor();
    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "start something",
            None,
        )
        .await
        .expect("the new-session send is accepted");
    let pending_id = send.session_id.clone();
    assert_eq!(
        ix.pending_session_ids().await,
        vec![pending_id.clone()],
        "the spawn is pending its first hook"
    );

    let err = ix
        .delete_session(&pending_id)
        .await
        .expect_err("a spawn awaiting its first hook must not be removed");
    assert!(
        matches!(&err, Error::SessionSpawning(id) if id == pending_id.as_str()),
        "a pending spawn is surfaced as SessionSpawning too, got {err:?}"
    );
    assert!(
        ix.store().session(&pending_id).await.unwrap().is_some(),
        "a refused removal deletes nothing"
    );
    assert_eq!(
        ix.pending_session_ids().await,
        vec![pending_id],
        "the pending spawn is left for its hook to bind, not consumed by the refusal"
    );
}
