use delta_model::AgentProvider;

use crate::error::Error;
use crate::interactor::testing::*;
use crate::SendTarget;

/// A launch option the provider's adapter refuses fails `POST /api/sends`
/// itself — the one adapter-decided failure that is still synchronous.
///
/// Everything else about an adapter-backed spawn moved behind the accept:
/// the worktree build, `connect` and `thread/start` all run on the launch task
/// and report as `spawn_failed`
/// (`a_failed_codex_launch_reaps_the_row_and_reports_spawn_failed`). Rendering
/// the selected options is different in kind: it is a pure function of the
/// request, so the accept phase can ask the adapter about it without
/// connecting, and the user gets the adapter's message on the send they just
/// made instead of a chip about a session that was created and torn down again.
///
/// The refusal lands before the eager row is written, so there is nothing to
/// roll back: no session, no launching entry, and no launch attempted at all —
/// the fake's `connect` would have succeeded, so an empty launch log is
/// evidence the gate fired first rather than that the launch failed anyway.
#[tokio::test]
async fn a_refused_codex_launch_option_fails_the_send_itself() {
    let factory = FakeAgentFactory::refusing_launch_option("thr_refused", "cwd");
    let ix = interactor_with_codex_factory(factory.clone());

    let refused = ix
        .store()
        .create_launch_option(
            None,
            "cwd",
            Some("/somewhere/else"),
            false,
            AgentProvider::Codex,
        )
        .await
        .unwrap();

    let err = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: vec![refused.id],
                worktree: None,
            },
            "hello codex",
            None,
        )
        .await
        .expect_err("an option the adapter refuses must fail the send");
    assert!(
        matches!(&err, Error::LaunchOptionRejected(message) if message.contains("cwd")),
        "the refusal keeps its own error kind (a `400` with the stable \
         `launch_option_rejected` code) and names the offending key, got {err:?}"
    );

    // Nothing was created and nothing was started: the gate runs before the
    // eager row, and no launch task was ever spawned.
    ix.await_launch().await;
    assert!(
        ix.store().inner.lock().unwrap().sessions.is_empty(),
        "a refused launch option leaves no session row"
    );
    assert!(
        ix.launching_session_ids().await.is_empty(),
        "no launch was recorded for a send that never got past the gate"
    );
    {
        let log = factory.log();
        let log = log.lock().unwrap();
        assert!(
            log.launches.is_empty(),
            "the adapter was never asked to launch"
        );
    }
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a Codex spawn never touches tmux"
    );
}
