//! A store error while the process-gone sweep clears one launch row must not
//! cost the other entries their `SubagentFinished`.
//!
//! The sweep drains the in-memory running-subagent set before it touches the
//! store, so an entry whose events are not returned is lost for good: nothing
//! re-lights it, and nothing would ever clear a live viewer's indicator for it.
//! The process is gone either way, so a failing clear is logged and the sweep
//! finishes every entry, the failing one included.

use std::collections::BTreeSet;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEndHook, SessionEvent, StopHook};

const SUBAGENTS: [&str; 3] = ["toolu_bg_1", "toolu_bg_2", "toolu_bg_3"];
const FAILING: &str = "toolu_bg_2";

#[tokio::test]
async fn session_end_sweep_carries_on_past_a_failing_launch_row_clear() {
    let (logs, _guard) = capture_warnings();
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // Three background subagents are launched and outlive their turn.
    for (index, tool_use_id) in SUBAGENTS.into_iter().enumerate() {
        ix.transcript_fake().push(background_tool_use_line(
            &format!("launch-{index}"),
            tool_use_id,
        ));
        ix.on_pre_tool_use(
            &session,
            "Agent",
            r#"{"subagent_type":"general-purpose","description":"Long crawl","run_in_background":true}"#,
            tool_use_id,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();
    }
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.running_subagents.len(),
        SUBAGENTS.len(),
        "all three background subagents survive the launching turn"
    );
    ix.store()
        .inner
        .lock()
        .unwrap()
        .fail_clear_subagent_launch_for = Some(FAILING.into());

    let events = ix
        .on_session_end(SessionEndHook {
            session_id: session.clone(),
            reason: Some("clear".into()),
        })
        .await
        .expect("a failing launch-row clear does not fail the session end");

    let finished: BTreeSet<&str> = events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::SubagentFinished {
                session_id,
                tool_use_id,
            } if *session_id == session => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        finished,
        BTreeSet::from(SUBAGENTS),
        "every swept subagent is announced finished, the failing one included: {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the running set is drained"
    );
    assert_eq!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .into_keys()
            .collect::<Vec<_>>(),
        vec![FAILING.to_owned()],
        "only the row whose clear failed is left behind"
    );
    let logged = logs.text();
    assert!(
        logged.contains("WARN")
            && logged.contains("session_id=sess-1")
            && logged.contains(&format!("tool_use_id={FAILING}"))
            && logged.contains("injected clear_subagent_launch failure"),
        "the failing clear is logged with the session, the subagent and the error: {logged}"
    );
}
