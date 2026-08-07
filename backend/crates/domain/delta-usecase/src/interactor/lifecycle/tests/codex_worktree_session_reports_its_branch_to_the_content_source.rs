use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A Codex session started in a git worktree hands its recorded launch site —
/// the worktree directory AND the branch it was created on — to the content
/// accumulator, which stamps both onto every message it folds.
///
/// The branch is the fact that can only come from Delta's own record: the Codex
/// app-server reports the thread's `cwd` but says nothing about git, and
/// re-deriving the branch later would drift from the `branch_at_launch` column
/// the session card already shows. So this asserts the two agree, at the seam
/// where the metadata leaves the core.
///
/// The worktree is resolved through the "branch already checked out" reuse path
/// so the launch directory is a fixed, scriptable path (a freshly created
/// worktree is named after the session id, which is minted mid-call).
#[tokio::test]
async fn codex_worktree_session_reports_its_branch_to_the_content_source() {
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

    // The session row recorded the worktree as its launch site.
    let session = ix.store().session(&send.session_id).await.unwrap().unwrap();
    assert_eq!(
        session.cwd, existing,
        "the session launched in the worktree"
    );
    assert_eq!(
        session.branch_at_launch.as_deref(),
        Some("pr-head"),
        "the worktree's branch was recorded on the session row"
    );

    // …and exactly that launch site reached the content accumulator, which is
    // what puts it on every message the session persists.
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
    assert_eq!(
        request.git_branch, session.branch_at_launch,
        "the content source stamps the same branch the session row records"
    );
}
