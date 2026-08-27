use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::SendTarget;

/// A Codex launch whose adapter connection fails reports itself, rather than
/// failing the request.
///
/// A connect failure (the provider binary is missing, the handshake is
/// rejected) used to be a synchronous `5xx` on `POST /api/sends`. Now the send
/// is accepted before the launch is attempted, so — exactly as on the Claude
/// path (`failed_launch_preparation_reaps_the_row_and_reports_spawn_failed`) —
/// the failure arrives on the async event seam as a `spawn_failed` carrying the
/// adapter's message as its `reason`, which is the only place that text can
/// still reach the user, and the eager row (with its first send, by cascade) is
/// deleted.
///
/// The `pane_token` is absent: a terminal-less session never had a pane, so
/// there is no name to report — the browser keys the failure on `session_id`
/// alone.
#[tokio::test]
async fn a_failed_codex_launch_reaps_the_row_and_reports_spawn_failed() {
    let factory = FakeAgentFactory::failing();
    let (ix, mut sink) = interactor_with_git_and_codex_factory_and_event_sink(
        FakeGitWorktree::default(),
        factory.clone(),
    );

    // The send is accepted with real ids: the connect has not been attempted.
    let (send, _) = ix
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
        .expect("the send is accepted before the adapter is connected");
    let session_id = send.session_id.clone();
    assert_ne!(send.id, 0, "the accepted send carries a real row id");

    ix.await_launch().await;

    // The failure is announced on the seam, naming its cause and no pane.
    let event = sink.try_recv().expect("a spawn failure was broadcast");
    let SessionEvent::SpawnFailed {
        session_id: failed_id,
        pane_token,
        reason,
        ..
    } = event
    else {
        panic!("expected SpawnFailed, got {event:?}");
    };
    assert_eq!(
        failed_id, session_id,
        "the failure names the accepted session"
    );
    assert_eq!(
        pane_token, None,
        "a terminal-less launch reports no pane token"
    );
    assert!(
        reason.is_some_and(|reason| reason.contains("fake codex connect failed")),
        "the failure carries the adapter's message, which no response body can carry now"
    );

    // The contentless row is gone, so the session stops being listed…
    assert!(
        ix.store().inner.lock().unwrap().sessions.is_empty(),
        "the eager Codex row of a failed launch is deleted"
    );
    // …and nothing was left running or launched: tmux is never touched by an
    // adapter-backed spawn, and the failed connect built no adapter, so no
    // handle dangles.
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a Codex spawn never touches tmux"
    );
    {
        let log = factory.log();
        let log = log.lock().unwrap();
        assert!(
            log.launches.is_empty() && log.closes == 0,
            "a connect that failed leaves no provider thread and nothing to close"
        );
    }
    assert!(
        ix.launching_session_ids().await.is_empty(),
        "the launching entry is settled, not left behind"
    );
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "an adapter-backed launch never becomes a pending spawn"
    );
    assert!(
        !ix.is_session_open(&session_id).await,
        "the failed launch left no live agent bound"
    );
}
