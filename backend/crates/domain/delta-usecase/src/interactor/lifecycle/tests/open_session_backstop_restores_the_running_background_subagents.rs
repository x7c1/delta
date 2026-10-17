//! The never-double-launch backstop adopts a closed session's live remembered
//! pane the way boot re-adoption does, so it rebuilds the running-subagent set
//! the same way: a background subagent launched before Delta lost track of the
//! session may still be running in that pane, and its launch line is behind
//! the cursor the catch-up starts from. Only the launches whose rows say they
//! lit the indicator come back — a foreground subagent leaves no row, and could
//! not have outlived its turn.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};
use crate::RunningSubagent;

use super::readoption_support::{left_behind, left_running, pane_for, survives, SURVIVING_TOKEN};

#[tokio::test]
async fn open_session_backstop_restores_a_background_subagent_but_not_a_foreground_one() {
    let (ix, mut async_events) = interactor_with_event_sink();

    // What the fold persists for each kind of launch: a row marked as lighting
    // the indicator for the background `Agent`, nothing for the foreground one.
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let folded = SessionId::from("sess-1");
    ix.transcript_fake().push(agent_tool_use_line(
        "a-bg",
        "toolu_bg",
        "Agent",
        "general-purpose",
        "Long crawl",
    ));
    ix.transcript_fake().push(foreground_agent_tool_use_line(
        "a-fg",
        "toolu_fg",
        "Agent",
        "Explore",
        "Quick look",
    ));
    ix.on_stop(StopHook {
        session_id: folded.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(
        ix.store()
            .running_subagent_launches(&folded)
            .await
            .unwrap()
            .into_iter()
            .map(|launch| launch.tool_use_id)
            .collect::<Vec<_>>(),
        vec!["toolu_bg".to_owned()],
        "only the background launch leaves a row a re-adoption can light"
    );
    assert!(!ix
        .store()
        .outstanding_subagent_launches(&folded)
        .await
        .unwrap()
        .contains_key("toolu_fg"));
    while async_events.try_recv().is_ok() {}

    // A closed session whose agent is still up in its remembered pane, left
    // with the same two launches: both launch lines are behind the cursor, and
    // only the background one has its row.
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    survives(&ix, SURVIVING_TOKEN);
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let child = ix
        .store()
        .create_thread(&id, "side topic", Some(main))
        .await
        .unwrap()
        .id;
    ix.transcript_fake().push_to(
        "/work/sess-A.jsonl",
        agent_tool_use_line("a-bg", "toolu_bg", "Agent", "general-purpose", "Long crawl"),
    );
    ix.transcript_fake().push_to(
        "/work/sess-A.jsonl",
        foreground_agent_tool_use_line("a-fg", "toolu_fg", "Agent", "Explore", "Quick look"),
    );
    ix.store().set_transcript_lines_read(&id, 3).await.unwrap();
    left_running(&ix, &id, "toolu_bg", child).await;
    assert!(!ix.is_session_open(&id).await);

    ix.open_session(&id).await.unwrap();

    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "the backstop adopted the live pane rather than resuming beside it"
    );
    assert_eq!(ix.bound_pane(&id).await, Some(pane_for(SURVIVING_TOKEN)));
    assert_eq!(
        ix.live_state_for(&id).await.running_subagents,
        vec![RunningSubagent {
            thread_id: child,
            tool_use_id: "toolu_bg".into(),
            task_id: Some("a31425032172620ed".into()),
            subagent_type: Some("general-purpose".into()),
            description: Some("Long crawl".into()),
            background: true,
        }],
        "the background subagent is running again on its launching thread, and the \
         foreground one is not"
    );
    let mut announced = Vec::new();
    while let Ok(event) = async_events.try_recv() {
        announced.push(event);
    }
    let started: Vec<_> = announced
        .iter()
        .filter(|event| matches!(event, SessionEvent::SubagentStarted { .. }))
        .collect();
    assert_eq!(
        started,
        vec![&SessionEvent::SubagentStarted {
            session_id: id.clone(),
            thread_id: child,
            tool_use_id: "toolu_bg".into(),
            subagent_type: Some("general-purpose".into()),
            description: Some("Long crawl".into()),
            background: true,
        }],
        "a connected viewer is told only the background subagent is running"
    );
}
