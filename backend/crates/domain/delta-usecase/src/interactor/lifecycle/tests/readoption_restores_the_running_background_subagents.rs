//! A background subagent keeps running in a pane that survives a restart of
//! Delta, but its launch line is behind the stored cursor the re-adoption's
//! catch-up starts from. The running set is rebuilt from the outstanding launch
//! rows instead, so the indicator stays lit until the subagent completes —
//! and only for the launches that lit it before the restart.

use delta_model::{ContentBlock, SessionId};

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};
use crate::{HookEndpointChange, RunningSubagent};

use super::readoption_support::{left_behind, left_running, survives, SURVIVING_TOKEN};

/// An assistant line launching `Bash` with `run_in_background: true`: it
/// leaves a launch row (its completion is routed back to its thread) but never
/// lights the running-subagent indicator.
fn background_bash_line(uuid: &str, tool_use_id: &str) -> delta_attribution::TranscriptMessage {
    delta_attribution::TranscriptMessage {
        content: vec![ContentBlock::ToolUse {
            id: tool_use_id.into(),
            name: "Bash".into(),
            input: serde_json::json!({ "command": "sleep 600", "run_in_background": true }),
        }],
        ..assistant_line(uuid, "")
    }
}

#[tokio::test]
async fn readoption_restores_a_background_subagent_launched_before_the_restart() {
    let (ix, mut async_events) = interactor_with_event_sink();
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    survives(&ix, SURVIVING_TOKEN);
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let child = ix
        .store()
        .create_thread(&id, "side topic", Some(main))
        .await
        .unwrap()
        .id;
    left_running(&ix, &id, "toolu_bg", child).await;

    ix.readopt_surviving_sessions(HookEndpointChange::Unchanged)
        .await
        .unwrap();

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
        "the sends envelope reports the subagent running on its launching thread"
    );
    let mut announced = Vec::new();
    while let Ok(event) = async_events.try_recv() {
        announced.push(event);
    }
    assert!(
        announced.contains(&SessionEvent::SubagentStarted {
            session_id: id.clone(),
            thread_id: child,
            tool_use_id: "toolu_bg".into(),
            subagent_type: Some("general-purpose".into()),
            description: Some("Long crawl".into()),
            background: true,
        }),
        "a connected viewer is told the subagent is running, got {announced:?}"
    );

    // It is a background entry like any other: a turn ending does not sweep
    // it, and its completion notification finishes it.
    ix.transcript_fake().push_to(
        "/work/sess-A.jsonl",
        task_notification_line("u-note", "toolu_bg"),
    );
    ix.on_stop(StopHook {
        session_id: id.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert!(ix.live_state_for(&id).await.running_subagents.is_empty());
    assert!(ix
        .store()
        .running_subagent_launches(&id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn readoption_leaves_finished_a_background_subagent_that_completed_while_delta_was_down() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    survives(&ix, SURVIVING_TOKEN);
    let main = ix.store().main_thread_id(&id).await.unwrap();
    left_running(&ix, &id, "toolu_bg", main).await;
    // The subagent finished while Delta was down: its notification is among
    // the lines the catch-up ingests.
    ix.transcript_fake().push_to(
        "/work/sess-A.jsonl",
        task_notification_line("u-note", "toolu_bg"),
    );

    ix.readopt_surviving_sessions(HookEndpointChange::Unchanged)
        .await
        .unwrap();

    assert!(
        ix.live_state_for(&id).await.running_subagents.is_empty(),
        "the completion the catch-up folded finished the restored entry"
    );
    assert!(
        ix.store()
            .outstanding_subagent_launches(&id)
            .await
            .unwrap()
            .is_empty(),
        "and consumed its launch row"
    );
}

/// The fold marks the launch row of a background `Agent` as lighting the
/// indicator, but not that of a background `Bash`, so a re-adoption lights the
/// one and not the other.
#[tokio::test]
async fn readoption_does_not_light_a_background_bash_launch() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    ix.transcript_fake()
        .push(background_tool_use_line("a-agent", "toolu_agent"));
    ix.transcript_fake()
        .push(background_bash_line("a-bash", "toolu_bash"));
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    let outstanding = ix
        .store()
        .outstanding_subagent_launches(&session)
        .await
        .unwrap();
    assert!(outstanding.contains_key("toolu_agent"));
    assert!(
        outstanding.contains_key("toolu_bash"),
        "the background Bash leaves a launch row for attributing its completion"
    );
    assert_eq!(
        ix.store()
            .running_subagent_launches(&session)
            .await
            .unwrap()
            .into_iter()
            .map(|launch| launch.tool_use_id)
            .collect::<Vec<_>>(),
        vec!["toolu_agent".to_owned()],
        "only the Agent launch is marked as lighting the indicator"
    );

    // A re-adopted session whose only outstanding launch is a background Bash.
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    survives(&ix, SURVIVING_TOKEN);
    let main = ix.store().main_thread_id(&id).await.unwrap();
    ix.store()
        .record_subagent_launch(&id, "toolu_bash", main)
        .await
        .unwrap();

    ix.readopt_surviving_sessions(HookEndpointChange::Unchanged)
        .await
        .unwrap();

    assert!(ix.is_session_open(&id).await);
    assert!(
        ix.live_state_for(&id).await.running_subagents.is_empty(),
        "a launch that never lit the indicator does not light it after a restart"
    );
}

/// A resume starts a new `claude` process, so the background subagents of the
/// previous one are gone: their launch rows are dropped, and a later
/// re-adoption of the resumed pane does not light them.
#[tokio::test]
async fn a_resume_drops_the_running_subagents_of_the_previous_process() {
    let ix = interactor();
    // The remembered pane did not survive, so opening the session resumes it.
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    let main = ix.store().main_thread_id(&id).await.unwrap();
    left_running(&ix, &id, "toolu_bg", main).await;

    ix.open_session(&id).await.unwrap();

    assert!(ix.live_state_for(&id).await.running_subagents.is_empty());
    assert!(
        ix.store()
            .running_subagent_launches(&id)
            .await
            .unwrap()
            .is_empty(),
        "the dead process's launch row no longer reads as running"
    );
}
