use delta_model::SessionId;

use crate::interactor::testing::*;

/// Resuming a Delta worktree session whose cwd — a git working tree under
/// Delta's own worktree base — is already trusted refreshes its pre-accepted
/// keys before launching `claude --resume` there, without asking again: trust
/// is per directory, and this one was decided at its launch.
#[tokio::test]
async fn open_session_refreshes_trust_for_an_already_trusted_worktree() {
    let wt = format!("{TEST_WORKTREE_BASE}/x7c1-delta-sess-R");
    let git = FakeGitWorktree::default()
        .with_repo(&wt, &wt)
        .with_trusted_dir(&wt);
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
        "the resumed, already-trusted worktree has its keys refreshed"
    );
}

/// Resuming in a worktree under the base that Claude Code does not trust
/// pre-accepts nothing: nothing about it was confirmed, so Claude Code's own
/// dialog applies.
#[tokio::test]
async fn open_session_does_not_pre_accept_an_untrusted_worktree() {
    let wt = format!("{TEST_WORKTREE_BASE}/x7c1-delta-sess-U");
    let git = FakeGitWorktree::default()
        .with_repo(&wt, &wt)
        .with_untrusted_repo(&wt);
    let ix = interactor_with_git(git);
    ix.on_user_prompt_submit(submit_in("sess-U", &format!("{wt}/t.jsonl"), &wt, "seed"))
        .await
        .unwrap();

    ix.open_session(&SessionId::from("sess-U")).await.unwrap();

    assert!(
        ix.git_worktree_fake().trusted.lock().unwrap().is_empty(),
        "an untrusted worktree is not pre-accepted on resume"
    );
}

/// Resuming a session whose cwd is NOT a git repository does not seed trust —
/// `repo_root` returns `None`, so the trust step is skipped.
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
