//! The bug: an open session whose `claude` went away without delivering a
//! `SessionEnd` hook — killed, crashed, its tmux session removed from outside —
//! keeps reading as open forever. Nothing re-checks the pane behind the
//! binding, so the list shows it open, the terminal attaches to nothing, every
//! send is typed into the dead pane and cancelled with an error, and a
//! background subagent stays lit because the only thing that clears it is a
//! process-gone sweep nothing has called.
//!
//! The liveness tick now probes the pane and closes the session when it is
//! gone, which is the whole fix: a *closed* session already behaves correctly.

use std::time::Instant;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};
use crate::turn::TurnState;

#[tokio::test]
async fn a_vanished_pane_closes_its_open_session() {
    let ix = interactor();

    // A real spawn, bound by its first hook: the pane `delta-1` exists in tmux
    // and the session is open on it.
    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(ix.is_session_open(&id).await, "open on its bound pane");

    // A background subagent is running, and its launching turn has ended — a
    // background entry outlives its turn, so only a process-gone sweep can
    // clear it.
    ix.transcript_fake().push_to(
        "/work/delta-1/t.jsonl",
        background_tool_use_line("a-1", "toolu_bg"),
    );
    ix.on_pre_tool_use(
        &id,
        "Agent",
        r#"{"subagent_type":"general-purpose","description":"Long crawl","run_in_background":true}"#,
        "toolu_bg",
        "/work/delta-1/t.jsonl",
    )
    .await
    .unwrap();
    ix.on_stop(StopHook {
        session_id: id.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // A turn is in flight when the process goes away (the shape that would
    // otherwise leave a permanent "in progress" chip).
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "and now this",
    ))
    .await
    .unwrap();
    assert_ne!(
        ix.live_state_for(&id).await.turn,
        TurnState::Idle,
        "a turn is in flight when the pane disappears"
    );

    // The tmux session is killed from outside Delta. No hook arrives.
    ix.tmux_fake().vanish_session("delta-1");

    let events = ix
        .reap_stale_spawns(Instant::now(), TICK_BOUND)
        .await
        .unwrap();

    assert!(
        !ix.is_session_open(&id).await,
        "the session whose pane is gone is closed on the tick"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            SessionEvent::SessionClosed { session_id } if *session_id == id
        )),
        "SessionClosed is reported so the browser refetches, got {events:?}"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            SessionEvent::SubagentFinished { session_id, tool_use_id }
                if *session_id == id && tool_use_id == "toolu_bg"
        )),
        "the lingering background subagent is swept, got {events:?}"
    );
    assert!(
        ix.live_state_for(&id).await.running_subagents.is_empty(),
        "no running-subagent indicator is left lit"
    );
    assert_eq!(
        ix.live_state_for(&id).await.turn,
        TurnState::Idle,
        "the in-flight turn is closed with the session"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "Delta does not kill a pane that is already gone"
    );
}
