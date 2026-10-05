use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::SendTarget;

/// An adapter-backed session with neither a repository nor a chosen directory
/// gets its per-session scratch directory created before the adapter starts
/// there, exactly as a Claude launch does.
#[tokio::test]
async fn a_codex_launch_without_a_repository_creates_its_scratch_workdir() {
    let factory = FakeAgentFactory::new("thr_fake", Some("turn_fake"));
    let ix = interactor_with_codex_factory(factory);

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello codex",
            None,
        )
        .await
        .unwrap();
    ix.await_launch().await;

    let session = ix.store().session(&send.session_id).await.unwrap().unwrap();
    assert_eq!(
        *ix.workspace_fake().created_dirs.lock().unwrap(),
        vec![session.cwd.clone()],
        "the session's directory is the one the workspace created"
    );
    assert_eq!(
        session.cwd,
        format!("{TEST_WORKDIR_BASE}/{}", send.session_id.as_str()),
        "<base>/<session id>"
    );
}
