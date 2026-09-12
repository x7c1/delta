use delta_model::SessionId;

use crate::interactor::testing::*;

/// Removing a closed session deletes Delta's rows and nothing else.
///
/// The store side of the cascade is pinned at the sqlite store
/// (`delete_session_cascades_to_children`); what this test is here to hold is
/// the promise the use case makes about everything *outside* the database: the
/// git worktree the session ran in (which may hold uncommitted work) and the
/// agent's own files are left exactly as they were. That is only observable as
/// "no gateway was asked to touch them", so the fakes' call logs are the
/// assertion.
#[tokio::test]
async fn delete_session_removes_a_closed_session_and_leaves_disk_alone() {
    let ix = interactor();
    // A known-but-closed session: it has a store row and no live pane, which is
    // the one state `Remove` is offered in.
    ix.on_user_prompt_submit(submit_in(
        "sess-closed",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-closed");
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");

    ix.delete_session(&id)
        .await
        .expect("a closed session can be removed");

    assert!(
        ix.store().session(&id).await.unwrap().is_none(),
        "the session row is gone, so the card leaves the list"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "a closed session has no pane, and removal never reaches for one"
    );
    assert!(
        ix.git_worktree_fake().created.lock().unwrap().is_empty()
            && ix
                .git_worktree_fake()
                .checked_out
                .lock()
                .unwrap()
                .is_empty(),
        "the worktree is not Delta's to destroy: removal touches no git gateway"
    );
    assert!(
        ix.workspace_fake().written.lock().unwrap().is_empty(),
        "nothing is written to the filesystem either"
    );
}
