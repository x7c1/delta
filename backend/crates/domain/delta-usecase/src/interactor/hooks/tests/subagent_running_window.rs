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
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let events = ix
        .on_pre_tool_use(&session, "Agent", AGENT_INPUT, "toolu_a1")
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
        "starting an Agent broadcasts SubagentStarted carrying its launching thread and labels"
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
        .on_post_tool_use(&session, "Agent", "toolu_a1", "null")
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
        .on_post_tool_use(&session, "Task", "toolu_t1", "null")
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
        .on_post_tool_use(&session, "Bash", "toolu_b1", "null")
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
    ix.on_post_tool_use(&session, "Agent", "toolu_a1", "null")
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
        .on_post_tool_use(&session, "Agent", "toolu_never_started", "null")
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
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let events = ix
        .on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg")
        .await
        .unwrap();

    assert_eq!(
        events,
        vec![SessionEvent::SubagentStarted {
            session_id: session.clone(),
            thread_id: main,
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
        .on_post_tool_use(&session, "Agent", "toolu_bg", "null")
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
    ix.on_post_tool_use(&session, "Agent", "toolu_bg", "null")
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

    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg")
        .await
        .unwrap();

    // The launch line is folded so the launch row exists in the store before
    // the immediate `PostToolUse` upgrades it.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // The background subagent's immediate `PostToolUse` carries the launch's
    // `agentId` in `tool_response`. The handler reads it, upgrades the running
    // entry, and persists the upgrade on the launch row.
    let events = ix
        .on_post_tool_use(
            &session,
            "Agent",
            "toolu_bg",
            POST_TOOL_USE_RESPONSE_WITH_AGENT_ID,
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

    // Launch a background subagent and persist its launch row.
    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg")
        .await
        .unwrap();
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // The PostToolUse upgrade records the task_id so the eventual
    // `<task-notification>` can be matched by it.
    ix.on_post_tool_use(
        &session,
        "Agent",
        "toolu_bg",
        POST_TOOL_USE_RESPONSE_WITH_AGENT_ID,
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
    ix.on_pre_tool_use(&session, "Agent", BACKGROUND_AGENT_INPUT, "toolu_bg")
        .await
        .unwrap();
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
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
