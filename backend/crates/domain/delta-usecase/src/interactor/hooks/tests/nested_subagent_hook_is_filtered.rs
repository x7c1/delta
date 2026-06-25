//! A nested subagent's `PreToolUse` / `PostToolUse` reaches the parent
//! session's hook endpoint (Claude Code dispatches it under the parent's
//! `session_id`) but its `transcript_path` points at the subagent's own
//! JSONL. The interactor must ignore that hook so the parent's running set
//! and event broadcast stay clean — otherwise a nested `Agent` launch would
//! light a running indicator that can never clear (its completion lands in
//! the subagent's transcript, which Delta does not tail for the parent).

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// A nested subagent's transcript lives under the parent session's transcript
/// directory: `<parent>.jsonl` is sibling to `<parent>/subagents/agent-…jsonl`.
/// Pick any path that differs from [`SEED_TRANSCRIPT_PATH`] (`/tmp/t.jsonl`) to
/// simulate the nested case.
const NESTED_TRANSCRIPT_PATH: &str = "/tmp/t/subagents/agent-deadbeef.jsonl";

const AGENT_INPUT: &str = r#"{"subagent_type":"general-purpose","description":"Long crawl","prompt":"…","run_in_background":true}"#;

#[tokio::test]
async fn pre_tool_use_against_a_nested_transcript_is_a_noop() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // Fire `PreToolUse` for an `Agent` launch whose `transcript_path` belongs
    // to a nested subagent — different from the session row's stored path.
    let events = ix
        .on_pre_tool_use(
            &session,
            "Agent",
            AGENT_INPUT,
            "toolu_nested",
            NESTED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "a nested-transcript PreToolUse must not broadcast any event, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "a nested-transcript PreToolUse must not add a running subagent entry"
    );
}

#[tokio::test]
async fn pre_tool_use_against_the_parent_transcript_still_starts_the_window() {
    // Symmetric positive path: with the seed transcript path, the same call
    // produces the existing `SubagentStarted` event and a running entry. The
    // guard is selective, not blanket.
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let events = ix
        .on_pre_tool_use(
            &session,
            "Agent",
            AGENT_INPUT,
            "toolu_parent",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::SubagentStarted { .. })),
        "a parent-transcript PreToolUse(Agent) still broadcasts SubagentStarted, got {events:?}"
    );
    let running: Vec<String> = ix
        .live_state_for(&session)
        .await
        .running_subagents
        .iter()
        .map(|s| s.tool_use_id.clone())
        .collect();
    assert_eq!(
        running,
        vec!["toolu_parent".to_owned()],
        "the parent-transcript subagent is the only running entry"
    );
}

#[tokio::test]
async fn post_tool_use_against_a_nested_transcript_does_not_clear_the_parents_running_entry() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // The parent session starts its own subagent.
    ix.on_pre_tool_use(
        &session,
        "Agent",
        AGENT_INPUT,
        "toolu_parent",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    // A nested subagent's `PostToolUse` arrives carrying the SAME `tool_use_id`
    // by accident, but its `transcript_path` is the nested transcript. The
    // guard must drop it before the running-subagent lookup, leaving the
    // parent's entry intact and broadcasting nothing.
    let events = ix
        .on_post_tool_use(
            &session,
            "Agent",
            "toolu_parent",
            "null",
            NESTED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "a nested-transcript PostToolUse must not broadcast any event, got {events:?}"
    );
    let running: Vec<String> = ix
        .live_state_for(&session)
        .await
        .running_subagents
        .iter()
        .map(|s| s.tool_use_id.clone())
        .collect();
    assert_eq!(
        running,
        vec!["toolu_parent".to_owned()],
        "the parent's running subagent must survive a nested-transcript PostToolUse"
    );
}
