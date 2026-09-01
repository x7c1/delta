//! The failed-resume twin of `session_end_sweeps_a_lingering_background_subagent`.
//!
//! A `SessionEnd` for a resume that never became ready tears the pane down and
//! reports `SpawnFailed`. That process is just as gone as a normal end's, so a
//! BACKGROUND subagent still running from a turn BEFORE the resume window can
//! never have its completion folded — the branch must run the same
//! process-gone sweep, or the indicator sticks forever.

use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEndHook, SessionEvent, StopHook};

#[tokio::test]
async fn session_end_for_failed_resume_sweeps_a_lingering_background_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // A background subagent is launched and outlives its turn.
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

    // The session is being resumed and has not become ready yet.
    ix.push_resuming_at("delta-3", &session, None, Instant::now())
        .await;
    ix.tmux_fake()
        .live
        .lock()
        .unwrap()
        .push("delta-3".to_owned());

    // The resume ends before readiness: a failed resume. Its process is gone,
    // so the lingering background entry is swept alongside the `SpawnFailed`.
    let events = ix
        .on_session_end(SessionEndHook {
            session_id: session.clone(),
            reason: Some("exit".into()),
        })
        .await
        .unwrap();

    assert!(
        events.iter().any(
            |e| matches!(e, SessionEvent::SpawnFailed { session_id: sid, .. } if *sid == session)
        ),
        "the failed resume is still reported, got {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentFinished { session_id: sid, tool_use_id }
                if *sid == session && tool_use_id == "toolu_bg"
        )),
        "the swept entry is broadcast as SubagentFinished, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the lingering background subagent is cleared on the failed resume's end"
    );
    assert!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .is_empty(),
        "the persisted launch row is cleared so a resume cannot resurrect the entry"
    );
}
