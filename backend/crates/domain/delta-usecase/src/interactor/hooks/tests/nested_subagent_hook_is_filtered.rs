//! A nested subagent's `Agent`/`Task` launch must not light a parent indicator
//! that can never clear (the completion lands in the subagent's JSONL, which
//! Delta does not tail for the parent).
//!
//! The mechanism that protects against this is the parent's transcript ingest:
//! the running-subagent indicator is now driven from
//! `Effect::SubagentIndicatorStarted`, which fires only when an `Agent`/`Task`
//! `tool_use` block is folded out of the PARENT's JSONL. A nested subagent's
//! `tool_use(Agent)` is written to the subagent's own JSONL, never the
//! parent's, so it cannot produce a parent indicator.
//!
//! The `is_foreign_transcript` short-circuit on the `PreToolUse` /
//! `PostToolUse` paths is retained for the permission-row path — if Claude
//! Code does reliably tag the nested call with the subagent's transcript path
//! the older guard still prevents a permission row from being attached to the
//! parent — and is exercised here by the `post_tool_use` test below.

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
async fn pre_tool_use_with_no_matching_parent_tool_use_line_creates_no_indicator() {
    // The exact regression scenario from real Claude Code 2.1.193: a nested
    // `Agent`/`Task` launch's `PreToolUse` arrives carrying the PARENT's
    // `transcript_path` — Claude Code presents nested hook metadata identically
    // to a parent-level launch — so the older transcript-path filter does NOT
    // catch it. The new design is structural: the parent's JSONL carries no
    // matching `tool_use(Agent)` block for a nested launch (it lives in the
    // subagent's own JSONL), so the indicator-from-ingest path is never
    // reached for a nested id. The hook does fire `sync_transcript`, but it
    // finds no Agent tool_use line, so no entry is added and no event fires.
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // The parent's JSONL only contains an unrelated Bash tool_use (or nothing
    // — the point is no Agent/Task tool_use). A nested Agent launch's
    // `PreToolUse` reaches the parent's hook endpoint with the PARENT's
    // transcript_path. With the new design, the hook syncs the parent's
    // transcript but finds no matching tool_use line, so the indicator stays
    // dark.
    ix.transcript_fake()
        .push(bash_tool_use_line("a-bash", "toolu_bash"));

    let events = ix
        .on_pre_tool_use(
            &session,
            "Agent",
            AGENT_INPUT,
            "toolu_nested",
            // Same `SEED_TRANSCRIPT_PATH` the parent session was registered
            // under — the realistic CC 2.1.193 shape where `transcript_path`
            // cannot distinguish a nested launch from a parent one.
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "a PreToolUse(Agent) without a matching tool_use line in the parent transcript \
         emits no SubagentStarted, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "no running entry is added when the parent transcript does not carry the launch"
    );
}

#[tokio::test]
async fn parent_jsonl_agent_tool_use_via_sync_lights_the_indicator() {
    // The positive path: when the PARENT's JSONL carries the `tool_use(Agent)`
    // block (whose presence is the authoritative signal), the indicator lights
    // up on the very next sync. PreToolUse force-syncs the parent transcript,
    // so this is the same hook-driven latency the older implementation had.
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_parent"));

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
        "a parent-JSONL tool_use(Agent) lights the indicator, got {events:?}"
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
        "the parent-JSONL subagent is the only running entry"
    );
}

#[tokio::test]
async fn post_tool_use_against_a_nested_transcript_does_not_clear_the_parents_running_entry() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // The parent session has a running subagent (from its own JSONL).
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_parent"));
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
    // by accident but its `transcript_path` is the nested transcript. The
    // `is_foreign_transcript` guard drops it before the running-subagent
    // lookup, leaving the parent's entry intact and broadcasting nothing.
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
