use crate::interactor::testing::*;
use crate::ports::BranchDeletion;
use crate::WorktreeBranch;

use super::support::{session_on, under_base, REPO_ROOT};

/// Each listed worktree carries the branch checked out in it, with what
/// decides its fate when its session is removed: only the `delta-<id>` branch
/// Delta cut for a listed session working there is one Delta may delete, and
/// only then — in a clean worktree — is git asked whether it is merged.
#[tokio::test]
async fn list_worktree_dirs_tells_which_branch_removing_its_session_would_delete() {
    let names = ["dirty", "leftover", "merged", "shared", "unmerged"];
    let [dirty, leftover, merged, shared, unmerged] = names.map(under_base);
    let mut git = FakeGitWorktree::default()
        .with_branch_deletion(
            "delta-unmerged",
            Scripted::Answer(BranchDeletion::KeptUnmerged),
        )
        .with_inspection(&dirty, REPO_ROOT, true)
        .with_current_branch(&dirty, "delta-dirty");
    for (path, branch) in [
        (&leftover, "delta-removed"),
        (&merged, "delta-merged"),
        (&shared, "feature"),
        (&unmerged, "delta-unmerged"),
    ] {
        git = git
            .with_inspection(path, REPO_ROOT, false)
            .with_current_branch(path, branch);
    }
    let ix = interactor_with_workspace_and_git(
        FakeWorkspace::default().with_children(TEST_WORKTREE_BASE, &names),
        git,
    );
    session_on(&ix, "dirty", &dirty, Some("delta-dirty")).await;
    session_on(&ix, "merged", &merged, Some("delta-merged")).await;
    // Started on a branch that already existed: never Delta's to delete.
    session_on(&ix, "shared", &shared, Some("feature")).await;
    session_on(&ix, "unmerged", &unmerged, Some("delta-unmerged")).await;

    let dirs = ix.list_worktree_dirs().await.unwrap();

    let branch = |name: &str, created_for_session, merged| {
        Some(WorktreeBranch {
            name: name.to_owned(),
            created_for_session,
            merged,
        })
    };
    let branches: Vec<_> = dirs.into_iter().map(|dir| dir.branch).collect();
    assert_eq!(
        branches,
        vec![
            branch("delta-dirty", true, None),
            branch("delta-removed", false, None),
            branch("delta-merged", true, Some(true)),
            branch("feature", false, None),
            branch("delta-unmerged", true, Some(false)),
        ]
    );
}
