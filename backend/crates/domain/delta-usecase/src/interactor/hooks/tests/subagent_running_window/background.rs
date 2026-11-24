//! Background subagents: launched entries that outlive their `PostToolUse` and the turn ending.

use delta_model::SessionId;

use super::*;
use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

#[tokio::test]
async fn a_background_launch_starts_a_background_running_entry() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));

    let events = ix
        .on_pre_tool_use(
            &session,
            "Agent",
            BACKGROUND_AGENT_INPUT,
            "toolu_bg",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    // The `background_tool_use_line` builder omits `description`, so only the
    // `subagent_type` label flows through. (The hook's `tool_input_json` no
    // longer feeds the indicator; the parent transcript ingest does, and the
    // ingested tool_use input is what carries the displayable fields.)
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentStarted {
                session_id,
                thread_id,
                tool_use_id,
                subagent_type,
                background,
                ..
            }
                if *session_id == session
                    && *thread_id == main
                    && tool_use_id == "toolu_bg"
                    && subagent_type.as_deref() == Some("general-purpose")
                    && *background
        )),
        "a `run_in_background` launch broadcasts SubagentStarted with background:true, got {events:?}"
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

    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        BACKGROUND_AGENT_INPUT,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    // A background launch's `PostToolUse` fires immediately (the call returned,
    // the subagent did not), so it must NOT finish the running entry.
    let events = ix
        .on_post_tool_use(&session, "Agent", "toolu_bg", "null", SEED_TRANSCRIPT_PATH)
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

    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        BACKGROUND_AGENT_INPUT,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    // Its immediate PostToolUse (a no-op for the indicator).
    ix.on_post_tool_use(&session, "Agent", "toolu_bg", "null", SEED_TRANSCRIPT_PATH)
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

    ix.transcript_fake().push(foreground_agent_tool_use_line(
        "a-launch-fg",
        "toolu_fg",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        FOREGROUND_AGENT_INPUT,
        "toolu_fg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch-bg", "toolu_bg"));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        BACKGROUND_AGENT_INPUT,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
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

#[tokio::test]
async fn agent_without_run_in_background_is_background_and_survives_post_tool_use() {
    // The bug this regression test pins: modern Claude Code (>= v2.1.193)
    // dropped the `run_in_background` parameter from the `Agent`/`Task` tool
    // schema and made those calls async by default. Before the predicate fix,
    // the parent-transcript fold treated the missing flag as foreground, so
    // the immediate `PostToolUse(Agent)` — which fires when the launch
    // returned, not when the subagent finished — closed the running window
    // within milliseconds and the navigator never showed the indicator.
    //
    // The end-to-end shape exercised here mirrors the real bug: a `tool_use`
    // input with no `run_in_background` key, then the immediate `PostToolUse`,
    // and the running entry must survive — exactly the path a background
    // subagent takes.
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The modern-shape Agent tool_use is in the parent transcript before
    // `PreToolUse(Agent)` fires (Claude Code's real ordering); the hook
    // force-syncs it.
    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch",
        "toolu_modern",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));

    let started = ix
        .on_pre_tool_use(
            &session,
            "Agent",
            AGENT_INPUT,
            "toolu_modern",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();
    assert_eq!(
        started,
        vec![SessionEvent::SubagentStarted {
            session_id: session.clone(),
            thread_id: main,
            tool_use_id: "toolu_modern".to_owned(),
            subagent_type: Some("general-purpose".to_owned()),
            description: Some("Run ls and count entries".to_owned()),
            background: true,
        }],
        "an Agent tool_use with no `run_in_background` key is broadcast as background"
    );

    // The immediate `PostToolUse(Agent)` fires (the launch returned, not the
    // subagent), and must NOT close the running window — that is the bug.
    let post = ix
        .on_post_tool_use(
            &session,
            "Agent",
            "toolu_modern",
            "null",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();
    assert!(
        post.is_empty(),
        "the immediate PostToolUse for a modern Agent broadcasts nothing"
    );
    let state = ix.live_state_for(&session).await;
    assert_eq!(
        running_tool_use_ids(&state),
        vec!["toolu_modern".to_owned()],
        "the modern Agent is still running after its immediate PostToolUse"
    );
    assert!(
        is_background(&state, "toolu_modern"),
        "the running entry is marked background"
    );
}
