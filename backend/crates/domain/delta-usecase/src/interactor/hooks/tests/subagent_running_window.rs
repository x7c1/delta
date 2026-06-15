//! The foreground subagent running window: `PreToolUse(Agent)` starts it,
//! `PostToolUse(Agent)` ends it, and the running set survives in queryable live
//! state so a reconnecting client rebuilds the indicator.
//!
//! Only `Agent`/`Task` flip the indicator — a subagent's nested tool calls
//! (e.g. its own `Bash`) reach the same hooks but must not — multiple
//! concurrent subagents are tracked independently, an unknown end is a no-op,
//! and a turn ending clears any still-running entry.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

const AGENT_INPUT: &str =
    r#"{"subagent_type":"general-purpose","description":"Run ls and count entries","prompt":"…"}"#;

/// An `Agent` launched with `run_in_background: true`: its `PostToolUse` fires
/// at launch (the call returned, not the subagent), and its completion arrives
/// later as a `<task-notification>`.
const BACKGROUND_AGENT_INPUT: &str = r#"{"subagent_type":"general-purpose","description":"Long crawl","prompt":"…","run_in_background":true}"#;

fn running_tool_use_ids(state: &crate::SessionLiveState) -> Vec<String> {
    state
        .running_subagents
        .iter()
        .map(|s| s.tool_use_id.clone())
        .collect()
}

fn is_background(state: &crate::SessionLiveState, tool_use_id: &str) -> bool {
    state
        .running_subagents
        .iter()
        .find(|s| s.tool_use_id == tool_use_id)
        .map(|s| s.background)
        .unwrap_or_else(|| panic!("no running subagent {tool_use_id}"))
}

#[tokio::test]
async fn pre_tool_use_agent_starts_the_window_and_broadcasts_with_display_fields() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let events = ix
        .on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1")
        .await
        .unwrap();

    assert_eq!(
        events,
        vec![SessionEvent::SubagentStarted {
            session_id: session.clone(),
            tool_use_id: "toolu_a1".to_owned(),
            subagent_type: Some("general-purpose".to_owned()),
            description: Some("Run ls and count entries".to_owned()),
            background: false,
        }],
        "starting an Agent broadcasts SubagentStarted carrying its labels"
    );

    let state = ix.live_state_for(&session).await;
    assert_eq!(
        running_tool_use_ids(&state),
        vec!["toolu_a1".to_owned()],
        "the subagent is in the queryable running set"
    );
}

#[tokio::test]
async fn post_tool_use_agent_ends_the_window_and_broadcasts() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1")
        .await
        .unwrap();

    let events = ix
        .on_post_tool_use(&session, "Agent", "toolu_a1")
        .await
        .unwrap();

    assert_eq!(
        events,
        vec![SessionEvent::SubagentFinished {
            session_id: session.clone(),
            tool_use_id: "toolu_a1".to_owned(),
        }],
        "completing the Agent broadcasts SubagentFinished"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the running set is empty once the subagent finished"
    );
}

#[tokio::test]
async fn the_task_alias_drives_the_same_window() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let started = ix
        .on_pre_tool_use(&session, "Task", AGENT_INPUT, "toolu_t1")
        .await
        .unwrap();
    assert!(
        matches!(started.as_slice(), [SessionEvent::SubagentStarted { .. }]),
        "the historical `Task` name starts a subagent too"
    );

    let finished = ix
        .on_post_tool_use(&session, "Task", "toolu_t1")
        .await
        .unwrap();
    assert!(
        matches!(finished.as_slice(), [SessionEvent::SubagentFinished { .. }]),
        "`Task` ends the window too"
    );
}

#[tokio::test]
async fn a_subagent_internal_tool_call_does_not_flip_the_indicator() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // Subagent running.
    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1")
        .await
        .unwrap();

    // A nested Bash call inside the subagent reaches the main session's hooks.
    // It must neither add a second running entry nor (on its Post) clear the
    // subagent's window.
    let bash_pre = ix
        .on_pre_tool_use(&session, "Bash", r#"{"command":"ls"}"#, "toolu_b1")
        .await
        .unwrap();
    assert!(
        bash_pre.is_empty(),
        "an internal Bash PreToolUse emits no subagent event"
    );
    let bash_post = ix
        .on_post_tool_use(&session, "Bash", "toolu_b1")
        .await
        .unwrap();
    assert!(
        bash_post.is_empty(),
        "an internal Bash PostToolUse emits no subagent event"
    );

    assert_eq!(
        running_tool_use_ids(&ix.live_state_for(&session).await),
        vec!["toolu_a1".to_owned()],
        "the subagent is still the only running entry; the nested Bash did not flip it"
    );
}

#[tokio::test]
async fn multiple_concurrent_subagents_are_tracked_independently() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1")
        .await
        .unwrap();
    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a2")
        .await
        .unwrap();
    assert_eq!(
        running_tool_use_ids(&ix.live_state_for(&session).await),
        vec!["toolu_a1".to_owned(), "toolu_a2".to_owned()],
        "both subagents are running, in start order"
    );

    // Finishing one leaves the other running.
    ix.on_post_tool_use(&session, "Agent", "toolu_a1")
        .await
        .unwrap();
    assert_eq!(
        running_tool_use_ids(&ix.live_state_for(&session).await),
        vec!["toolu_a2".to_owned()],
        "only the finished subagent is cleared"
    );
}

#[tokio::test]
async fn post_tool_use_for_an_unknown_subagent_is_a_noop() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // No matching PreToolUse was ever recorded for this id.
    let events = ix
        .on_post_tool_use(&session, "Agent", "toolu_never_started")
        .await
        .unwrap();
    assert!(
        events.is_empty(),
        "an end for an untracked subagent emits nothing"
    );
}

#[tokio::test]
async fn a_duplicate_pre_tool_use_does_not_double_track_or_double_broadcast() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1")
        .await
        .unwrap();
    let again = ix
        .on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1")
        .await
        .unwrap();

    assert!(
        again.is_empty(),
        "a retried PreToolUse for the same id re-broadcasts nothing"
    );
    assert_eq!(
        running_tool_use_ids(&ix.live_state_for(&session).await),
        vec!["toolu_a1".to_owned()],
        "the subagent is tracked exactly once"
    );
}

#[tokio::test]
async fn the_turn_ending_clears_a_still_running_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1")
        .await
        .unwrap();
    assert!(
        !ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the subagent is running before the turn ends"
    );

    // The turn ends (Stop hook) before any PostToolUse arrived: a subagent
    // cannot outlive its turn, so the running set is swept.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "a running subagent never outlives its turn"
    );
}

#[tokio::test]
async fn a_background_launch_starts_a_background_running_entry() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let events = ix
        .on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg")
        .await
        .unwrap();

    assert_eq!(
        events,
        vec![SessionEvent::SubagentStarted {
            session_id: session.clone(),
            tool_use_id: "toolu_bg".to_owned(),
            subagent_type: Some("general-purpose".to_owned()),
            description: Some("Long crawl".to_owned()),
            background: true,
        }],
        "a `run_in_background` launch broadcasts SubagentStarted with background:true"
    );

    let state = ix.live_state_for(&session).await;
    assert_eq!(running_tool_use_ids(&state), vec!["toolu_bg".to_owned()]);
    assert!(
        is_background(&state, "toolu_bg"),
        "the running entry is marked background"
    );
}

#[tokio::test]
async fn the_immediate_post_tool_use_does_not_finish_a_background_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg")
        .await
        .unwrap();

    // A background launch's `PostToolUse` fires immediately (the call returned,
    // the subagent did not), so it must NOT finish the running entry.
    let events = ix
        .on_post_tool_use(&session, "Agent", "toolu_bg")
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "the immediate PostToolUse for a background subagent broadcasts nothing"
    );
    assert_eq!(
        running_tool_use_ids(&ix.live_state_for(&session).await),
        vec!["toolu_bg".to_owned()],
        "the background subagent is still running after its immediate PostToolUse"
    );
}

#[tokio::test]
async fn a_background_subagent_survives_the_turn_ending() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg")
        .await
        .unwrap();
    // Its immediate PostToolUse (a no-op for the indicator).
    ix.on_post_tool_use(&session, "Agent", "toolu_bg")
        .await
        .unwrap();

    // The launching turn ends. A background subagent outlives the turn that
    // launched it, so the turn-end sweep must keep it.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    let state = ix.live_state_for(&session).await;
    assert_eq!(
        running_tool_use_ids(&state),
        vec!["toolu_bg".to_owned()],
        "the background subagent survives the turn ending"
    );
    assert!(is_background(&state, "toolu_bg"));
}

#[tokio::test]
async fn a_foreground_and_a_background_subagent_diverge_at_turn_end() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_fg")
        .await
        .unwrap();
    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg")
        .await
        .unwrap();
    assert_eq!(
        running_tool_use_ids(&ix.live_state_for(&session).await),
        vec!["toolu_fg".to_owned(), "toolu_bg".to_owned()],
    );

    // The turn ends: the foreground entry is swept, the background one survives.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    assert_eq!(
        running_tool_use_ids(&ix.live_state_for(&session).await),
        vec!["toolu_bg".to_owned()],
        "only the foreground subagent is swept at turn end"
    );
}
