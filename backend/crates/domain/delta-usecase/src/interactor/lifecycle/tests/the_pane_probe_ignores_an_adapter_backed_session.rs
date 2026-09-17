//! An adapter-backed (Codex) session is open without a pane, and its adapter
//! already reports process exit on its own seam (`AgentEvent::SessionEnded`,
//! which settles the session — see `codex_session_death_settles_the_session`).
//! The pane probe keys on the bound *pane handle* precisely so it cannot reach
//! such a session: probing tmux for a token it never had could only ever answer
//! "gone", closing a perfectly live Codex session on every tick.

use std::time::Instant;

use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::SendTarget;

#[tokio::test]
async fn the_pane_probe_ignores_an_adapter_backed_session() {
    let factory = FakeAgentFactory::new("thr_probe", Some("turn_probe"));
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
    let session_id = send.session_id.clone();
    assert!(
        ix.is_session_open(&session_id).await,
        "open on its adapter, with no pane"
    );

    let events = ix
        .reap_stale_spawns(Instant::now(), TICK_BOUND)
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "a terminal-less session produces no close, got {events:?}"
    );
    assert!(
        ix.is_session_open(&session_id).await,
        "the Codex session is still open after the tick"
    );
}
