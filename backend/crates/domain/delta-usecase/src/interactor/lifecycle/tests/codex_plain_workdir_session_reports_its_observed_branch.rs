use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::SendTarget;

/// A Codex session started in a plain git directory — **no worktree** — reports
/// that directory's branch on the messages it persists.
///
/// This is the case the metadata work exists for. Delta only fills the session
/// row's `branch_at_launch` on the worktree spawn path, and Codex's
/// `thread/start` response reports no git metadata at all (`gitInfo` is null),
/// so neither of the obvious sources knows the branch here. Delta observes it
/// from the launch directory at bind time instead, which is what this pins.
///
/// The session row is deliberately left alone: `branch_at_launch` stays NULL for
/// a non-worktree spawn, exactly as before, because it is a different fact (a
/// spawn-time snapshot feeding the session card) and changing it would alter
/// what the navigator shows.
#[tokio::test]
async fn codex_plain_workdir_session_reports_its_observed_branch() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, "/projects/app/.git/..")
        .with_current_branch(&canonical, "feature/plain-workdir");
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
                // No worktree: the plain-directory case.
                worktree: None,
            },
            "work here",
            None,
        )
        .await
        .expect("a Codex session starts in the plain workdir");
    ix.await_launch().await;

    let request = {
        let log = factory.log();
        let log = log.lock().unwrap();
        log.content_requests[0].clone()
    };
    assert_eq!(
        request.cwd,
        canonical.as_str(),
        "the content source is given the launch directory"
    );
    assert_eq!(
        request.git_branch.as_deref(),
        Some("feature/plain-workdir"),
        "the branch observed in the launch directory reaches the content source, \
         even though this spawn recorded no branch_at_launch"
    );

    // The session row is untouched by the observation: this spawn is not a
    // worktree spawn, so its snapshot column stays NULL as it always has.
    let session = ix.store().session(&send.session_id).await.unwrap().unwrap();
    assert_eq!(
        session.branch_at_launch, None,
        "observing the branch for messages must not start filling the session \
         row's spawn-time snapshot"
    );
}

/// A directory that is not a git working tree (or has a detached HEAD) reports
/// no branch, rather than one being invented — the fake resolves `current_branch`
/// to `None` for any directory it was not scripted with, mirroring the real
/// gateway's behaviour in both cases.
#[tokio::test]
async fn a_session_outside_a_git_working_tree_reports_no_branch() {
    let git = FakeGitWorktree::default();
    let factory = FakeAgentFactory::new("thr_fake", Some("turn_fake"));
    let ix = interactor_with_git_and_codex_factory(git, factory.clone());

    ix.enqueue_send(
        SendTarget::NewSession {
            provider: AgentProvider::Codex,
            workdir: None,
            launch_option_ids: Vec::new(),
            worktree: None,
        },
        "work here",
        None,
    )
    .await
    .expect("a Codex session starts in the default scratch dir");
    ix.await_launch().await;

    let request = {
        let log = factory.log();
        let log = log.lock().unwrap();
        log.content_requests[0].clone()
    };
    assert_eq!(
        request.git_branch, None,
        "no branch is reported for a directory that is not a git working tree"
    );
}

/// A `git` that fails outright degrades to "no branch" instead of failing the
/// spawn.
///
/// The distinction matters: `Ok(None)` (not a repo, detached HEAD) and an error
/// (git missing or broken) are different conditions, but they have the same
/// honest answer for a message — no branch. Propagating the error would instead
/// destroy a session the user asked for, over a piece of decorative metadata.
#[tokio::test]
async fn a_failing_git_degrades_the_branch_instead_of_failing_the_spawn() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, "/projects/app/.git/..")
        .with_failing_current_branch();
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
                worktree: None,
            },
            "work here",
            None,
        )
        .await
        .expect("a broken git must not fail the spawn");
    ix.await_launch().await;

    let request = {
        let log = factory.log();
        let log = log.lock().unwrap();
        log.content_requests[0].clone()
    };
    assert_eq!(
        request.git_branch, None,
        "a git failure reports no branch rather than propagating"
    );

    // The session is real and usable: the failure cost only the metadata.
    let session = ix.store().session(&send.session_id).await.unwrap().unwrap();
    assert_eq!(session.provider, AgentProvider::Codex);
}
