use crate::interactor::testing::*;
use crate::WorktreeDir;

use super::support::{session_in, under_base, REPO_ROOT};

/// Every directory under the base is listed, spelled under the configured
/// base (not the canonical form the directory listing returns), with whether
/// a listed session works in it and what git says of it.
#[tokio::test]
async fn list_worktree_dirs_marks_the_ones_a_session_works_in() {
    let (owned, clean, dirty, gone) = (
        under_base("owned"),
        under_base("clean"),
        under_base("dirty"),
        under_base("gone"),
    );
    let ix = interactor_with_workspace_and_git(
        FakeWorkspace::default()
            .with_children(TEST_WORKTREE_BASE, &["clean", "dirty", "gone", "owned"]),
        FakeGitWorktree::default()
            .with_inspection(&owned, REPO_ROOT, false)
            .with_inspection(&clean, REPO_ROOT, false)
            .with_inspection(&dirty, REPO_ROOT, true),
    );
    session_in(&ix, "sess-1", &owned).await;

    let dirs = ix.list_worktree_dirs().await.unwrap();

    let registered = |path: &str, in_use, dirty| WorktreeDir {
        path: path.to_owned(),
        in_use,
        repo_root: Some(REPO_ROOT.to_owned()),
        dirty: Some(dirty),
    };
    assert_eq!(
        dirs,
        vec![
            registered(&clean, false, false),
            registered(&dirty, false, true),
            WorktreeDir {
                path: gone,
                in_use: false,
                repo_root: None,
                dirty: None,
            },
            registered(&owned, true, false),
        ]
    );
}
