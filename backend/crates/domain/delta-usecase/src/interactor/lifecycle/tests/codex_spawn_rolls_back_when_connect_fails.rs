use delta_model::AgentProvider;

use crate::error::Error;
use crate::interactor::testing::*;
use crate::SendTarget;

/// A Codex spawn whose adapter connection fails (e.g. the provider binary is
/// unavailable) surfaces the error and leaves no orphan: the eagerly-inserted
/// session row is rolled back, exactly as a failed tmux launch rolls back a
/// Claude spawn.
#[tokio::test]
async fn codex_spawn_rolls_back_when_connect_fails() {
    let factory = FakeAgentFactory::failing();
    let ix = interactor_with_codex_factory(factory);

    let err = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hi",
            None,
        )
        .await
        .expect_err("a failed connect must surface as an error");
    assert!(
        matches!(err, Error::Agent(_)),
        "a connect failure surfaces as an agent error, got {err:?}"
    );

    // The eager session row was rolled back: nothing persisted, nothing spawned.
    assert!(
        ix.store().inner.lock().unwrap().sessions.is_empty(),
        "the eager Codex session row is rolled back on connect failure"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a Codex spawn never touches tmux"
    );
}
