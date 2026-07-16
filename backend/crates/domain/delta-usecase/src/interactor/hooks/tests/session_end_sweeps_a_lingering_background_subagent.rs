//! Variant 2 of the stuck-indicator bug: a BACKGROUND subagent whose parent
//! `claude` process ends (a normal `SessionEnd`) before its completion
//! `<task-notification>` was folded.
//!
//! The indicator is lit from parent-transcript ingest and a background entry is
//! normally cleared only by folding its notification. Once the process is gone
//! no more transcript is ingested, so that notification can never arrive — the
//! entry would stick forever. `on_session_end`'s normal-end path sweeps it,
//! broadcasting `SubagentFinished` and clearing the persisted launch row.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEndHook, SessionEvent, StopHook};

#[tokio::test]
async fn session_end_sweeps_a_lingering_background_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // A background subagent is launched: the parent's JSONL carries the
    // `tool_use(Agent)` block, and `PreToolUse(Agent)` force-syncs it so the
    // indicator lights and the launch row is persisted on the same hook call.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    let started = ix
        .on_pre_tool_use(
            &session,
            "Agent",
            r#"{"subagent_type":"general-purpose","description":"Long crawl","run_in_background":true}"#,
            "toolu_bg",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();
    assert!(
        started.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentStarted {
                background: true,
                ..
            }
        )),
        "the launch started a background running entry, got {started:?}"
    );

    // The launching turn stops. A background subagent outlives its turn, so the
    // turn-end sweep must KEEP it — the regression this guards against.
    let after_stop = ix
        .on_stop(StopHook {
            session_id: session.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();
    assert!(
        !after_stop
            .iter()
            .any(|e| matches!(e, SessionEvent::SubagentFinished { .. })),
        "Stop must NOT finish the background subagent, got {after_stop:?}"
    );
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

    // The `claude` process ends normally. Its completion notification can never
    // arrive now, so the normal-end path sweeps the lingering entry.
    let events = ix
        .on_session_end(SessionEndHook {
            session_id: session.clone(),
            reason: Some("clear".into()),
        })
        .await
        .unwrap();

    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentFinished { session_id: sid, tool_use_id }
                if *sid == session && tool_use_id == "toolu_bg"
        )),
        "SessionEnd broadcasts SubagentFinished for the swept entry, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the lingering background subagent is cleared on session end"
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
}
