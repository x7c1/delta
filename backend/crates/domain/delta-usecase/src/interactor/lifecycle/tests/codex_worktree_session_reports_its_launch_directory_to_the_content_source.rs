use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A Codex session started in a git worktree hands the **worktree** directory to
/// the content accumulator, which stamps it onto every message it folds.
///
/// The distinction this pins is between the two directories a worktree spawn
/// holds: the dir the user picked (recorded as `requested_workdir`) and the
/// worktree the agent was actually launched in (recorded as `cwd`). Messages
/// must report the latter — it is where the agent's edits land and where the
/// transcript's "open this directory" action should go.
///
/// The branch is deliberately NOT asserted here: it does not travel through the
/// core at all. The Codex server reports the branch it observed in the thread's
/// working directory, so it is read off the `thread/start` response by the
/// adapter and covered by the adapter/full-loop tests instead.
///
/// The worktree is resolved through the "branch already checked out" reuse path
/// so the launch directory is a fixed, scriptable path (a freshly created
/// worktree is named after the session id, which is minted mid-call).
#[tokio::test]
async fn codex_worktree_session_reports_its_launch_directory_to_the_content_source() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    // `pr-head` is already checked out at this path, so the spawn reuses it as
    // the launch directory instead of creating a new worktree.
    let existing = "/worktrees/app-pr-head";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git")
        .with_branch_checked_out("pr-head", existing)
        .with_current_branch(existing, "pr-head");
    let factory = FakeAgentFactory::new("thr_fake", Some("turn_fake"));
    let ix = interactor_with_git_and_codex_factory(git, factory.clone());
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let (send, _events) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: AgentProvider::Codex,
                workdir: Some("/projects/app".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: Some(WorktreeSpec {
                    start_point: WorktreeStartPoint::UseRemoteBranch("pr-head".to_owned()),
                }),
            },
            "work on this branch",
            None,
        )
        .await
        .expect("a Codex session starts in the reused worktree");
    ix.await_launch().await;

    // The session row recorded the worktree as its launch directory, distinct
    // from the dir the user selected.
    let session = ix.store().session(&send.session_id).await.unwrap().unwrap();
    assert_eq!(
        session.cwd, existing,
        "the session launched in the worktree"
    );
    assert_eq!(
        session.requested_workdir.as_deref(),
        Some(canonical.as_str()),
        "the user-selected dir is recorded separately, and is NOT the launch dir"
    );

    // …and exactly that launch directory reached the content accumulator, which
    // is what puts it on every message the session persists.
    let request = {
        let log = factory.log();
        let log = log.lock().unwrap();
        assert_eq!(
            log.content_requests.len(),
            1,
            "one content source was built for the spawn"
        );
        log.content_requests[0].clone()
    };
    assert_eq!(
        request.cwd, session.cwd,
        "the content source stamps the same cwd the session row records"
    );
}
