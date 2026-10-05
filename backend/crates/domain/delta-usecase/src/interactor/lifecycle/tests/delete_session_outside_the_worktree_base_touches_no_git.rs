use crate::interactor::testing::*;

use super::removal_support::closed_session;

/// Removing a session that did not run in a worktree Delta created deletes
/// Delta's rows and nothing else.
///
/// The store side of the cascade is pinned at the sqlite store
/// (`delete_session_cascades_to_children`); what this test holds is the
/// promise about everything *outside* the database. A session that ran in the
/// user's own repository — outside `worktree_base` — is the user's directory,
/// not Delta's, so no git gateway is asked to touch it (and no pane, and no
/// file), and the removal reports nothing removed or kept.
#[tokio::test]
async fn delete_session_outside_the_worktree_base_touches_no_git() {
    let ix = interactor();
    let id = closed_session(
        &ix,
        "sess-closed",
        "/home/u/repos/delta",
        Some("/home/u/repos/delta"),
        Some("main"),
    )
    .await;

    let removal = ix
        .delete_session(&id)
        .await
        .expect("a closed session can be removed");

    assert!(
        ix.store().session(&id).await.unwrap().is_none(),
        "the session row is gone, so the card leaves the list"
    );
    assert!(
        removal.removed.is_empty() && removal.kept.is_empty(),
        "nothing on disk was Delta's to consider, got {removal}"
    );
    assert!(
        ix.git_worktree_fake().removal_untouched(),
        "the user's own directory is not Delta's to remove: git is not asked"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "a closed session has no pane, and removal never reaches for one"
    );
    assert!(
        ix.workspace_fake().written.lock().unwrap().is_empty(),
        "nothing is written to the filesystem either"
    );
}
