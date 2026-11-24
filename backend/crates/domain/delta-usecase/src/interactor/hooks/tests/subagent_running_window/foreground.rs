//! Foreground subagents: the window `PreToolUse(Agent)` lights, `PostToolUse(Agent)` closes, and the turn-end sweep clears.

use delta_model::SessionId;

use super::*;
use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

#[tokio::test]
async fn pre_tool_use_foreground_agent_starts_the_window_and_broadcasts_with_display_fields() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The assistant message containing the `Agent` tool_use block has been
    // flushed to the parent's JSONL before `PreToolUse(Agent)` fires (this is
    // Claude Code's real ordering). `on_pre_tool_use` force-syncs the parent
    // transcript, so the indicator lights on the same hook call. This case
    // pins the explicit-foreground shape (the explicit `run_in_background:
    // false` shape), so the broadcast carries `background: false`.
    ix.transcript_fake().push(foreground_agent_tool_use_line(
        "a-launch",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));

    let events = ix
        .on_pre_tool_use(
            &session,
            "Agent",
            FOREGROUND_AGENT_INPUT,
            "toolu_a1",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert_eq!(
        events,
        vec![SessionEvent::SubagentStarted {
            session_id: session.clone(),
            thread_id: main,
            tool_use_id: "toolu_a1".to_owned(),
            subagent_type: Some("general-purpose".to_owned()),
            description: Some("Run ls and count entries".to_owned()),
            background: false,
        }],
        "the parent-transcript Agent tool_use lights the indicator with its labels"
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

    // Parent transcript carries the foreground launch line; PreToolUse syncs
    // it. The explicit `run_in_background: false` is what keeps the call
    // foreground under the modern async-by-default semantics, so the matching
    // PostToolUse closes the indicator window.
    ix.transcript_fake().push(foreground_agent_tool_use_line(
        "a-launch",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        FOREGROUND_AGENT_INPUT,
        "toolu_a1",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    let events = ix
        .on_post_tool_use(&session, "Agent", "toolu_a1", "null", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();

    assert_eq!(
        events,
        vec![SessionEvent::SubagentFinished {
            session_id: session.clone(),
            tool_use_id: "toolu_a1".to_owned(),
        }],
        "completing the foreground Agent broadcasts SubagentFinished"
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

    ix.transcript_fake().push(foreground_agent_tool_use_line(
        "a-launch",
        "toolu_t1",
        "Task",
        "general-purpose",
        "Run ls and count entries",
    ));
    let started = ix
        .on_pre_tool_use(
            &session,
            "Task",
            FOREGROUND_AGENT_INPUT,
            "toolu_t1",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();
    assert!(
        matches!(started.as_slice(), [SessionEvent::SubagentStarted { .. }]),
        "the historical `Task` name starts a subagent too"
    );

    let finished = ix
        .on_post_tool_use(&session, "Task", "toolu_t1", "null", SEED_TRANSCRIPT_PATH)
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
    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        AGENT_INPUT,
        "toolu_a1",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    // A nested Bash call inside the subagent reaches the main session's hooks.
    // The parent's JSONL never carries that Bash tool_use, so even with the
    // PreToolUse-force-sync nothing new lights up. (`Bash` is not a subagent
    // tool, so the sync is also skipped at the hook layer.)
    let bash_pre = ix
        .on_pre_tool_use(
            &session,
            "Bash",
            r#"{"command":"ls"}"#,
            "toolu_b1",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();
    assert!(
        bash_pre.is_empty(),
        "an internal Bash PreToolUse emits no subagent event"
    );
    let bash_post = ix
        .on_post_tool_use(&session, "Bash", "toolu_b1", "null", SEED_TRANSCRIPT_PATH)
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

    // Both launches are foreground so PostToolUse closes them: that lets the
    // test pin "finishing one leaves the other running" via PostToolUse alone.
    ix.transcript_fake().push(foreground_agent_tool_use_line(
        "a-launch-1",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        FOREGROUND_AGENT_INPUT,
        "toolu_a1",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    ix.transcript_fake().push(foreground_agent_tool_use_line(
        "a-launch-2",
        "toolu_a2",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        FOREGROUND_AGENT_INPUT,
        "toolu_a2",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    assert_eq!(
        running_tool_use_ids(&ix.live_state_for(&session).await),
        vec!["toolu_a1".to_owned(), "toolu_a2".to_owned()],
        "both subagents are running, in start order"
    );

    // Finishing one leaves the other running.
    ix.on_post_tool_use(&session, "Agent", "toolu_a1", "null", SEED_TRANSCRIPT_PATH)
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

    // No tool_use line was folded for this id.
    let events = ix
        .on_post_tool_use(
            &session,
            "Agent",
            "toolu_never_started",
            "null",
            SEED_TRANSCRIPT_PATH,
        )
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

    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        AGENT_INPUT,
        "toolu_a1",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    // A retried PreToolUse for the same id: the second sync sees no new lines
    // (the cursor advanced) so no second event is emitted.
    let again = ix
        .on_pre_tool_use(
            &session,
            "Agent",
            AGENT_INPUT,
            "toolu_a1",
            SEED_TRANSCRIPT_PATH,
        )
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
async fn the_turn_ending_clears_a_still_running_foreground_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // Explicit-foreground launch (`run_in_background: false`): the turn-end
    // sweep is what should clear it, since a foreground subagent cannot outlive
    // its launching turn. (A modern async-by-default launch would survive the
    // sweep — see `agent_without_run_in_background_is_background_and_survives_post_tool_use`
    // in `background.rs`.)
    ix.transcript_fake().push(foreground_agent_tool_use_line(
        "a-launch",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        FOREGROUND_AGENT_INPUT,
        "toolu_a1",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    assert!(
        !ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the subagent is running before the turn ends"
    );

    // The turn ends (Stop hook) before any PostToolUse arrived: a foreground
    // subagent cannot outlive its turn, so the running set is swept.
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
        "a running foreground subagent never outlives its turn"
    );
}
