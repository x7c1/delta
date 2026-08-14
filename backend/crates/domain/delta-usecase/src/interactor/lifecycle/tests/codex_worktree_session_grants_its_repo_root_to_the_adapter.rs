//! The wiring half of the worktree sandbox grant: which sessions tell their
//! adapter that their working directory is a Delta-created worktree, and out of
//! which repository.
//!
//! What the adapter *does* with that fact is Codex's business and is pinned in
//! the `codex-agent` crate (it becomes a `sandbox_workspace_write.writable_roots`
//! entry for the repository's `.git`, which is where a linked worktree's writes
//! actually land). What is pinned here is that the fact travels at all — on the
//! launch AND on the resume — because an adapter that is told `None` produces a
//! perfectly valid request that silently brings the approval prompts back.

use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A Codex session launched in a Delta-created worktree tells the adapter the
/// repository that worktree was cut from — and tells it again on a resume, which
/// re-derives the same value from the persisted session row rather than
/// remembering anything from the launch.
///
/// The resume half is the one worth the extra setup: nothing in the process
/// survives a restart, so if the fact were not re-derived from the row, a
/// resumed worktree session would quietly lose its sandbox grant while looking
/// identical from the outside.
///
/// The worktree is resolved through the "branch already checked out" reuse path
/// so the launch directory is a fixed, scriptable path (a freshly created
/// worktree is named after the session id, which is minted mid-call).
#[tokio::test]
async fn codex_worktree_session_grants_its_repo_root_to_the_adapter() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let existing = "/worktrees/app-pr-head";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git")
        .with_branch_checked_out("pr-head", existing)
        .with_current_branch(existing, "pr-head");
    let factory = FakeAgentFactory::new("thr_worktree", Some("turn_fake"));
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
    let session_id = send.session_id.clone();

    {
        let log = factory.log();
        let log = log.lock().unwrap();
        assert_eq!(log.launches.len(), 1, "one launch for the spawn");
        assert_eq!(
            log.launches[0].workdir, existing,
            "the agent was launched in the worktree"
        );
        assert_eq!(
            log.launches[0].worktree_repo_root.as_deref(),
            Some(repo_root),
            "the launch names the repository the worktree was cut from — the one \
             holding the git directory its writes go through"
        );
    }

    // Simulate a restart: drop the in-process binding, leaving only the
    // persisted row. The next send must reconnect over `resume`.
    ix.with_runtime(&session_id, |state| {
        let _ = state.remove_open_agent();
    })
    .await;

    let main_thread = ix.store().main_thread_id(&session_id).await.unwrap();
    ix.enqueue_send(
        SendTarget::Thread {
            thread_id: main_thread,
            branch_from: None,
        },
        "carry on",
        None,
    )
    .await
    .expect("the closed Codex session resumes over the adapter");

    let log = factory.log();
    let log = log.lock().unwrap();
    assert_eq!(log.resumes.len(), 1, "one resume for the reconnect");
    assert_eq!(
        log.resumes[0].workdir, existing,
        "the resume reattaches in the same worktree"
    );
    assert_eq!(
        log.resumes[0].worktree_repo_root.as_deref(),
        Some(repo_root),
        "the resume re-derives the same repository from the session row, so a \
         reattached worktree session is configured exactly like a fresh one"
    );
}

/// A Codex session in a plain git directory — the user's own clone, no worktree
/// — names no repository, so its adapter request is unchanged by this feature.
///
/// Its `.git` is not somewhere else: it sits inside the working directory the
/// agent was pointed at. Whether that directory should be writable is the user's
/// own sandbox configuration to make, and Delta claiming it here would silently
/// widen every ordinary session's sandbox.
#[tokio::test]
async fn a_plain_codex_session_names_no_repository() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, "/projects/app/.git/..")
        .with_current_branch(&canonical, "main");
    let factory = FakeAgentFactory::new("thr_plain", Some("turn_fake"));
    let ix = interactor_with_git_and_codex_factory(git, factory.clone());
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    ix.enqueue_send(
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

    let log = factory.log();
    let log = log.lock().unwrap();
    assert_eq!(
        log.launches[0].workdir,
        canonical.as_str(),
        "the agent was launched in the directory the user picked"
    );
    assert_eq!(
        log.launches[0].worktree_repo_root, None,
        "a plain launch claims no repository, even though the directory IS a git \
         repository — the grant is only for worktrees Delta created"
    );
}
