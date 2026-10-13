use crate::interactor::testing::*;
use crate::ports::BranchDeletion;
use crate::WorktreeBranch;

use super::support::{session_on, under_base, REPO_ROOT};

/// A session can check out another branch in the worktree Delta cut for it.
/// Removing it still settles its `delta-<id>` branch, so the listing names
/// that branch beside the checked-out one — with whether it is merged when
/// the worktree is clean — unless it is the one checked out or no longer
/// exists.
#[tokio::test]
async fn list_worktree_dirs_tells_the_branch_its_session_switched_away_from() {
    let names = [
        "x7c1-delta-gone",
        "x7c1-delta-on-it",
        "x7c1-delta-switched",
        "x7c1-delta-switched-dirty",
    ];
    let [gone, on_it, switched, switched_dirty] = names.map(under_base);
    let mut git = FakeGitWorktree::default()
        .with_branch_deletion("delta-gone", Scripted::Answer(BranchDeletion::Absent))
        .with_branch_deletion(
            "delta-switched",
            Scripted::Answer(BranchDeletion::KeptUnmerged),
        )
        .with_inspection(&switched_dirty, REPO_ROOT, true)
        .with_current_branch(&switched_dirty, "feature-dirty");
    for (path, branch) in [
        (&gone, "feature-gone"),
        (&on_it, "delta-on-it"),
        (&switched, "feature"),
    ] {
        git = git
            .with_inspection(path, REPO_ROOT, false)
            .with_current_branch(path, branch);
    }
    let ix = interactor_with_workspace_and_git(
        FakeWorkspace::default().with_children(TEST_WORKTREE_BASE, &names),
        git,
    );
    for (id, path) in [
        ("gone", &gone),
        ("on-it", &on_it),
        ("switched", &switched),
        ("switched-dirty", &switched_dirty),
    ] {
        session_on(&ix, id, path, Some(&format!("delta-{id}"))).await;
    }

    let dirs = ix.list_worktree_dirs().await.unwrap();

    let launch = |name: &str, merged| {
        Some(WorktreeBranch {
            name: name.to_owned(),
            created_for_session: true,
            merged,
        })
    };
    let launch_branches: Vec<_> = dirs.into_iter().map(|dir| dir.launch_branch).collect();
    assert_eq!(
        launch_branches,
        vec![
            None,
            None,
            launch("delta-switched", Some(false)),
            launch("delta-switched-dirty", None),
        ]
    );
}
