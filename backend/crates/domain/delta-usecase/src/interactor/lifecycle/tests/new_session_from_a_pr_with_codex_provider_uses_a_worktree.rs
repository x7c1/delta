use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// Starting a Codex session from a selected PR works: the send carries the
/// PR's head branch as a `UseRemoteBranch` worktree request (exactly what the
/// PR tab pre-fills) together with `provider: Codex`, and the Codex launch path
/// now honors it instead of rejecting the worktree.
///
/// This is the regression the branch fixes — before, the Codex spawn path returned
/// `Error::Agent("a git worktree is not supported for a Codex session")` for
/// any worktree spec, so a Codex + PR-origin start failed outright. The
/// worktree is just a working directory, so the fix resolves it through the
/// same path Claude uses (`resolve_worktree_launch_dir`) and launches the Codex
/// adapter there.
#[tokio::test]
async fn new_session_from_a_pr_with_codex_provider_uses_a_worktree() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    // The PR head branch is not checked out anywhere yet (the fresh-review
    // case): absent from the checked-out map, so the worktree is created via
    // `add_worktree_checkout`.
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git");
    let factory = FakeAgentFactory::new("thr_fake", Some("turn_fake"));
    let ix = interactor_with_git_and_codex_factory(git, factory.clone());
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let (send, events) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: AgentProvider::Codex,
                workdir: Some("/projects/app".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: Some(WorktreeSpec {
                    start_point: WorktreeStartPoint::UseRemoteBranch("pr-head".to_owned()),
                }),
            },
            "resume this PR on codex",
            None,
        )
        .await
        .expect("a Codex session must start from a PR-origin worktree, not error");
    ix.await_launch().await;
    assert!(
        events.is_empty(),
        "no synchronous events from a codex create"
    );

    let session_id = send.session_id.clone();
    let expected_path = format!("{TEST_WORKTREE_BASE}/x7c1-delta-{}", session_id.as_str());

    // A checkout worktree was added for the PR branch at the per-session path;
    // no new-branch worktree was created.
    assert!(
        ix.git_worktree_fake().created.lock().unwrap().is_empty(),
        "the use-branch mode adds a checkout, never a new `delta-<id>` branch"
    );
    let checked_out = ix.git_worktree_fake().checked_out.lock().unwrap().clone();
    assert_eq!(
        checked_out.len(),
        1,
        "one checkout worktree added for the PR branch"
    );
    assert_eq!(checked_out[0].repo_root, repo_root);
    assert_eq!(checked_out[0].worktree_path, expected_path);
    assert_eq!(checked_out[0].branch, "pr-head");

    // Terminal-less: a Codex session never spawns a tmux pane, even from a PR.
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a Codex session must not spawn a tmux pane"
    );

    // The session row is Codex, activated, and its cwd is the PR worktree. The
    // repo-identity columns are filled from the worktree's repository (the
    // navigator card's repo line), mirroring the Claude worktree path.
    let session = ix.store().session(&session_id).await.unwrap().unwrap();
    assert_eq!(session.provider, AgentProvider::Codex);
    assert_eq!(
        session.cwd, expected_path,
        "the Codex cwd is the PR worktree"
    );
    assert_eq!(session.repo_root.as_deref(), Some(repo_root));
    assert_eq!(
        session.repository_display_name.as_deref(),
        Some("x7c1/delta"),
        "the worktree's repo identity is recorded for the navigator card"
    );
    assert_eq!(
        session.requested_workdir.as_deref(),
        Some(canonical.as_str()),
        "the user-selected clone dir (canonicalized) is recorded as the requested workdir"
    );

    // The adapter was driven: the launch (thread/start) happened in the PR
    // worktree, and the first prompt reached the adapter's send.
    let (launch_workdir, sends) = {
        let log = factory.log();
        let log = log.lock().unwrap();
        (log.launches[0].workdir.clone(), log.sends.clone())
    };
    assert_eq!(
        launch_workdir, expected_path,
        "the Codex adapter launched in the PR worktree directory"
    );
    assert_eq!(sends, vec!["resume this PR on codex".to_owned()]);
}
