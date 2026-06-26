//! The subagent running-indicator window, now driven by the parent's
//! transcript ingest.
//!
//! Lighting the indicator is no longer a side effect of `PreToolUse(Agent)`:
//! the parent's JSONL contains the assistant message whose `tool_use(Agent)`
//! block is the authoritative signal, and `sync_transcript` emits
//! `Effect::SubagentIndicatorStarted` for it (see
//! [`delta_attribution::attribute_lines`]). `PreToolUse(Agent)` then force-syncs
//! the parent transcript so the indicator lights with the same low latency as
//! before. The mechanism naturally excludes nested subagents: a nested
//! `tool_use(Agent)` lands in the SUBAGENT's JSONL, never the parent's, so the
//! parent's ingest cannot accidentally produce a stuck indicator (the
//! depth>=2 regression PR #190's `transcript_path` filter could not catch on
//! Claude Code 2.1.193).
//!
//! Foreground vs background and the turn-end sweep are unchanged.

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
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The assistant message containing the `Agent` tool_use block has been
    // flushed to the parent's JSONL before `PreToolUse(Agent)` fires (this is
    // Claude Code's real ordering). `on_pre_tool_use` force-syncs the parent
    // transcript, so the indicator lights on the same hook call.
    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));

    let events = ix
        .on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1", SEED_TRANSCRIPT_PATH)
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

    // Parent transcript carries the launch line; PreToolUse syncs it.
    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1", SEED_TRANSCRIPT_PATH)
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

    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch",
        "toolu_t1",
        "Task",
        "general-purpose",
        "Run ls and count entries",
    ));
    let started = ix
        .on_pre_tool_use(&session, "Task", AGENT_INPUT, "toolu_t1", SEED_TRANSCRIPT_PATH)
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
    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();

    // A nested Bash call inside the subagent reaches the main session's hooks.
    // The parent's JSONL never carries that Bash tool_use, so even with the
    // PreToolUse-force-sync nothing new lights up. (`Bash` is not a subagent
    // tool, so the sync is also skipped at the hook layer.)
    let bash_pre = ix
        .on_pre_tool_use(&session, "Bash", r#"{"command":"ls"}"#, "toolu_b1", SEED_TRANSCRIPT_PATH)
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

    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch-1",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();
    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch-2",
        "toolu_a2",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a2", SEED_TRANSCRIPT_PATH)
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
        .on_post_tool_use(&session, "Agent", "toolu_never_started", "null", SEED_TRANSCRIPT_PATH)
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
    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();
    // A retried PreToolUse for the same id: the second sync sees no new lines
    // (the cursor advanced) so no second event is emitted.
    let again = ix
        .on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1", SEED_TRANSCRIPT_PATH)
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

    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch",
        "toolu_a1",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1", SEED_TRANSCRIPT_PATH)
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
        "a running subagent never outlives its turn"
    );
}

#[tokio::test]
async fn a_background_launch_starts_a_background_running_entry() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));

    let events = ix
        .on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg", SEED_TRANSCRIPT_PATH)
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
    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg", SEED_TRANSCRIPT_PATH)
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
    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg", SEED_TRANSCRIPT_PATH)
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

    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch-fg",
        "toolu_fg",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_fg", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch-bg", "toolu_bg"));
    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg", SEED_TRANSCRIPT_PATH)
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

/// A background `Agent` whose `PostToolUse(Agent)` reported the launch's
/// `agentId` as `tool_response.agentId`: the upgrade is persisted on both the
/// runtime entry and the launch store row, so a later
/// `<task-notification>` whose `<tool-use-id>` element was stripped can still
/// match by `<task-id>`.
const POST_TOOL_USE_RESPONSE_WITH_AGENT_ID: &str = r#"{"agentId":"a31425032172620ed"}"#;

#[tokio::test]
async fn post_tool_use_upgrades_the_background_subagent_with_its_agent_id() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();

    // The background subagent's immediate `PostToolUse` carries the launch's
    // `agentId` in `tool_response`. The handler reads it, upgrades the running
    // entry, and persists the upgrade on the launch row (created when PreToolUse
    // force-synced the parent's tool_use line above).
    let events = ix
        .on_post_tool_use(
            &session,
            "Agent",
            "toolu_bg",
            POST_TOOL_USE_RESPONSE_WITH_AGENT_ID,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "an immediate background PostToolUse broadcasts nothing"
    );
    let state = ix.live_state_for(&session).await;
    let running = state
        .running_subagents
        .iter()
        .find(|s| s.tool_use_id == "toolu_bg")
        .expect("background subagent is still running");
    assert_eq!(
        running.task_id.as_deref(),
        Some("a31425032172620ed"),
        "the running entry was upgraded with the agentId"
    );
    let launch_task_id = ix
        .store()
        .outstanding_subagent_launches(&session)
        .await
        .unwrap()
        .get("toolu_bg")
        .and_then(|launch| launch.task_id.clone());
    assert_eq!(
        launch_task_id.as_deref(),
        Some("a31425032172620ed"),
        "the upgrade is persisted on the launch row too"
    );
}

#[tokio::test]
async fn a_task_notification_missing_tool_use_id_finishes_via_the_task_id_fallback() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // Launch a background subagent. The parent's JSONL carries the tool_use
    // line; PreToolUse force-syncs it, so the launch row exists; the immediate
    // PostToolUse then upgrades the launch row with the agentId.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();
    ix.on_post_tool_use(
        &session,
        "Agent",
        "toolu_bg",
        POST_TOOL_USE_RESPONSE_WITH_AGENT_ID,
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    // The completion notification arrives with ONLY `<task-id>` — Claude Code
    // stripped `<tool-use-id>` from the user-message body. The server must
    // still finish the background subagent via the fallback correlation.
    ix.transcript_fake().push(task_notification_line_task_id_only(
        "u-note",
        "a31425032172620ed",
    ));
    let events = ix
        .on_stop(StopHook {
            session_id: session.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();

    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentFinished { session_id, tool_use_id }
                if *session_id == session && tool_use_id == "toolu_bg"
        )),
        "the task-id-only notification still emits SubagentFinished, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the running subagent was finished via the task-id fallback"
    );
    assert!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .is_empty(),
        "the launch correlation was cleared"
    );
}

#[tokio::test]
async fn a_task_notification_missing_both_ids_leaves_the_subagent_running_and_warns() {
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt;

    // Capture warn-level tracing output into a buffer so the test can assert
    // the warn fires when a `<task-notification>` body carries neither
    // correlation element. The subscriber is installed only for the duration
    // of this test (via the `_guard` returned by `set_default`), so it does
    // not leak across tests — the guard is held until the test ends.
    #[derive(Clone, Default)]
    struct BufferWriter(Arc<Mutex<Vec<u8>>>);
    impl Write for BufferWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> fmt::MakeWriter<'a> for BufferWriter {
        type Writer = BufferWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    let buffer = BufferWriter::default();
    let subscriber = fmt()
        .with_writer(buffer.clone())
        .with_max_level(tracing::Level::WARN)
        .without_time()
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // Launch a background subagent and persist its launch row.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();

    // A notification body without either `<tool-use-id>` or `<task-id>` —
    // a future Claude Code shape — must not silently drop the subagent:
    // the entry stays running and the fold logs a warn so we notice.
    ix.transcript_fake()
        .push(task_notification_line_both_missing("u-note"));
    let events = ix
        .on_stop(StopHook {
            session_id: session.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::SubagentFinished { .. })),
        "no SubagentFinished should fire when neither correlation key is present, got {events:?}"
    );
    assert_eq!(
        running_tool_use_ids(&ix.live_state_for(&session).await),
        vec!["toolu_bg".to_owned()],
        "the running entry survives a no-key notification — its completion is unknown"
    );

    let captured = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    assert!(
        captured.contains("WARN") && captured.contains("<task-notification>"),
        "expected a WARN log for the missing-keys notification, got: {captured}"
    );
}
