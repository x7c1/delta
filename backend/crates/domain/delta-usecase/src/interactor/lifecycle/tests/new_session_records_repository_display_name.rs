use crate::interactor::testing::*;
use crate::SendTarget;

/// A fresh session in a git-repo workdir with a configured `origin` URL
/// captures a short `org/repo` repository identity label on the session row.
/// The navigator's repo line renders this directly. Unlike `repo_root` —
/// which is the working-tree path itself when launched from a linked
/// worktree — this label is stable across worktrees of the same clone
/// because `origin` lives in the shared `.git/config`.
#[tokio::test]
async fn new_session_records_repository_display_name_from_origin_url() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, "/projects/app")
        .with_current_branch(&canonical, "feat/widget")
        .with_origin_url(&canonical, "https://github.com/x7c1/delta.git");
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                workdir: Some("/projects/app".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .unwrap();

    let session = ix
        .store()
        .session(&send.session_id)
        .await
        .unwrap()
        .expect("the eager spawning row was written");
    assert_eq!(
        session.repository_display_name.as_deref(),
        Some("x7c1/delta"),
        "the navigator card's repo label is captured at spawn from the origin URL",
    );
}

/// A fresh session in a git-repo workdir with NO configured `origin` URL
/// falls back to the working-tree basename for `repository_display_name` —
/// a local-only clone is still identifiable on the navigator card.
#[tokio::test]
async fn new_session_records_repository_display_name_from_basename_when_origin_unset() {
    let canonical = FakeWorkspace::canonical("/projects/local-only");
    // `with_repo` only — no `with_origin_url`, so `origin_url` resolves to
    // `None` like a fresh `git init` with no remote configured.
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, "/projects/local-only")
        .with_current_branch(&canonical, "main");
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/local-only".to_owned());

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                workdir: Some("/projects/local-only".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .unwrap();

    let session = ix
        .store()
        .session(&send.session_id)
        .await
        .unwrap()
        .expect("the eager spawning row was written");
    assert_eq!(
        session.repository_display_name.as_deref(),
        Some("local-only"),
        "a local-only repo falls back to the working-tree basename",
    );
}

/// A fresh session launched outside any git repository records
/// `repository_display_name` as `None`. The navigator's frontend then falls
/// back to the cwd basename for the repo line.
#[tokio::test]
async fn new_session_in_a_non_git_dir_records_no_repository_display_name() {
    let ix = interactor();
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/scratch".to_owned());

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                workdir: Some("/scratch".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .unwrap();

    let session = ix
        .store()
        .session(&send.session_id)
        .await
        .unwrap()
        .expect("the eager spawning row was written");
    assert!(
        session.repository_display_name.is_none(),
        "a non-git launch dir records no repository display name",
    );
}
