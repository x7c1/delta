//! Variant 2 of the stuck-indicator bug, via the other process-gone signal:
//! `close_session`. Closing tears down the `claude` process, so a lingering
//! BACKGROUND subagent's completion `<task-notification>` can no longer be
//! folded to clear its indicator. `close_session` sweeps it, returning a
//! `SubagentFinished` per entry (which the transport broadcasts) and clearing
//! the persisted launch row.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

#[tokio::test]
async fn close_session_sweeps_a_lingering_background_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    // Bind a live pane so the close exercises the real open→closed teardown.
    ix.bind_open_session("delta-seed", &session).await;

    // Launch a background subagent (lit by parent-transcript ingest, launch row
    // persisted on the same `PreToolUse(Agent)` force-sync).
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        r#"{"subagent_type":"general-purpose","description":"Long crawl","run_in_background":true}"#,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    // The launching turn stops. A background subagent outlives its turn, so it
    // must still be running after the Stop — the regression guard.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .iter()
            .map(|s| s.tool_use_id.clone())
            .collect::<Vec<_>>(),
        vec!["toolu_bg".to_owned()],
        "the background subagent survives the launching turn going idle"
    );

    // Close the session: the process is torn down, so the sweep runs and returns
    // the SubagentFinished events for the transport to broadcast.
    let events = ix.close_session(&session).await.unwrap();

    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentFinished { session_id: sid, tool_use_id }
                if *sid == session && tool_use_id == "toolu_bg"
        )),
        "close_session returns SubagentFinished for the swept entry, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the lingering background subagent is cleared on close"
    );
    assert!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .is_empty(),
        "the persisted launch row is cleared so a stray notification cannot \
         double-fire and a resume cannot resurrect the entry"
    );
    assert!(ix.pane_for_session(&session).await.is_none(), "closed");
}
