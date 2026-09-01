use delta_model::SessionId;

use crate::interactor::testing::*;

/// Resuming a Delta worktree session — a git-repository cwd under Delta's own
/// worktree base — pre-accepts the workspace-trust dialog for that cwd before
/// launching `claude --resume` there, so the resumed pane is not blocked on the
/// trust dialog.
#[tokio::test]
async fn open_session_seeds_trust_when_cwd_is_a_git_repo() {
    // The session's cwd is a git repository under the worktree base (a worktree
    // Delta created), which the fake reports a root for.
    let wt = format!("{TEST_WORKTREE_BASE}/x7c1-delta-sess-R");
    let git = FakeGitWorktree::default().with_repo(&wt, &wt);
    let ix = interactor_with_git(git);
    ix.on_user_prompt_submit(submit_in("sess-R", &format!("{wt}/t.jsonl"), &wt, "seed"))
        .await
        .unwrap();
    let id = SessionId::from("sess-R");

    ix.open_session(&id).await.unwrap();

    let trusted = ix.git_worktree_fake().trusted.lock().unwrap().clone();
    assert_eq!(
        trusted,
        vec![wt],
        "the resumed Delta-worktree cwd is seeded for trust"
    );
}

/// Resuming a session whose cwd is NOT a git repository (the default scratch
/// dir) does not seed trust — `repo_root` returns `None`, so the trust step is
/// skipped.
#[tokio::test]
async fn open_session_does_not_seed_trust_when_cwd_is_not_a_git_repo() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");

    ix.open_session(&id).await.unwrap();

    assert!(
        ix.git_worktree_fake().trusted.lock().unwrap().is_empty(),
        "a non-git cwd is not seeded for trust on resume"
    );
}
