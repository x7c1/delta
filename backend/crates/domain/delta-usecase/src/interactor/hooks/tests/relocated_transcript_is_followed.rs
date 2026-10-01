//! A session whose transcript Claude Code moves keeps being followed.
//!
//! On entering a worktree, Claude Code moves the session's JSONL whole to the
//! project directory of the new working directory, appends a `relocated` line
//! naming that directory, and reports the new path in every later hook. The
//! session's own hooks must re-point it at the new file and keep reading from
//! the same line cursor; a subagent's hooks, whose transcripts live under the
//! session's `subagents/` directory, must still be ignored.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

/// The session's transcript before the move: `<project dir>/<session id>.jsonl`.
const OLD_PATH: &str = "/projects/-work/sess-1.jsonl";
/// Where Claude Code moves it: the same file name under the project directory
/// of the worktree the session entered.
const NEW_PATH: &str = "/projects/-work-wt/sess-1.jsonl";
/// The worktree the `relocated` line names.
const NEW_CWD: &str = "/work/wt";
/// A subagent's transcript, in the session's own directory.
const SUBAGENT_PATH: &str = "/projects/-work/sess-1/subagents/agent-a1b2c3.jsonl";

fn sess() -> SessionId {
    SessionId::from("sess-1")
}

fn stop() -> StopHook {
    StopHook {
        session_id: sess(),
        stop_reason: None,
    }
}

/// Register `sess-1` on [`OLD_PATH`] and ingest a first exchange (two lines),
/// leaving the line cursor at 2.
async fn session_with_one_exchange(ix: &TestInteractor) {
    ix.on_user_prompt_submit(submit_in("sess-1", OLD_PATH, "/work", "seed"))
        .await
        .unwrap();
    ix.transcript_fake()
        .push_to(OLD_PATH, user_line("u-0", "seed"));
    ix.transcript_fake()
        .push_to(OLD_PATH, assistant_line("a-0", "first reply"));
    ix.on_stop(stop()).await.unwrap();
    assert_eq!(ix.store().message_count(&sess()).await.unwrap(), 2);
    assert_eq!(ix.store().transcript_lines_read(&sess()).await.unwrap(), 2);
}

async fn stored_transcript_path(ix: &TestInteractor) -> Option<String> {
    ix.store()
        .session(&sess())
        .await
        .unwrap()
        .unwrap()
        .transcript_path
}

#[tokio::test]
async fn a_relocated_transcript_is_followed_and_ingested_without_duplicates() {
    let ix = interactor();
    session_with_one_exchange(&ix).await;

    // `EnterWorktree` runs: the file moves (the old path stops existing) and
    // the `relocated` line lands at its end.
    ix.transcript_fake().relocate(OLD_PATH, NEW_PATH, NEW_CWD);

    // A sync before any hook names the new path reads the moved-away file as
    // empty. That must not pull the cursor back, or the lines already
    // ingested would be read again once the session follows the move.
    ix.on_stop(stop()).await.unwrap();
    assert_eq!(ix.store().transcript_lines_read(&sess()).await.unwrap(), 2);

    // The tool's `PostToolUse` is the first hook to report the new path.
    ix.on_post_tool_use(&sess(), "EnterWorktree", "toolu_wt", "null", NEW_PATH)
        .await
        .unwrap();

    let session = ix.store().session(&sess()).await.unwrap().unwrap();
    assert_eq!(session.transcript_path.as_deref(), Some(NEW_PATH));
    assert_eq!(
        session.cwd, NEW_CWD,
        "the session takes the working directory the relocated line names"
    );
    assert_eq!(
        ix.store().transcript_lines_read(&sess()).await.unwrap(),
        2,
        "the moved file continues the old one, so the cursor carries over"
    );

    // A reply written after the move is ingested from the new file, and the
    // lines read before the move are not read again.
    ix.transcript_fake()
        .push_to(NEW_PATH, assistant_line("a-1", "reply from the worktree"));
    ix.on_stop(stop()).await.unwrap();

    assert_eq!(
        ix.store().message_count(&sess()).await.unwrap(),
        3,
        "only the new reply was added"
    );
    // Two old lines, the `relocated` line, and the new reply.
    assert_eq!(ix.store().transcript_lines_read(&sess()).await.unwrap(), 4);
    let main = ix.store().main_thread_id(&sess()).await.unwrap();
    let texts: Vec<_> = ix
        .store()
        .thread_messages(main)
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.uuid.as_str().to_owned())
        .collect();
    assert_eq!(texts, ["u-0", "a-0", "a-1"]);
}

/// A live session left behind on a moved-away path before Delta followed
/// moves — its cursor pulled back to 0 by the tail reading the missing file —
/// is healed by its next hook: reading resumes right after the last line it
/// had ingested, so nothing before it is read again.
#[tokio::test]
async fn a_session_stuck_on_a_moved_away_path_is_healed_by_its_next_hook() {
    let ix = interactor();
    session_with_one_exchange(&ix).await;
    ix.transcript_fake().relocate(OLD_PATH, NEW_PATH, NEW_CWD);
    ix.store()
        .set_transcript_lines_read(&sess(), 0)
        .await
        .unwrap();

    ix.on_user_prompt_submit(submit_in("sess-1", NEW_PATH, NEW_CWD, "next"))
        .await
        .unwrap();

    assert_eq!(stored_transcript_path(&ix).await.as_deref(), Some(NEW_PATH));
    // Past the two ingested lines and the `relocated` line.
    assert_eq!(ix.store().transcript_lines_read(&sess()).await.unwrap(), 3);
    assert_eq!(ix.store().message_count(&sess()).await.unwrap(), 2);
}

#[tokio::test]
async fn a_subagent_hook_is_still_foreign_and_does_not_repoint_the_session() {
    let ix = interactor();
    session_with_one_exchange(&ix).await;

    let events = ix
        .on_pre_tool_use(
            &sess(),
            "Bash",
            r#"{"command":"ls"}"#,
            "toolu_nested",
            SUBAGENT_PATH,
        )
        .await
        .unwrap();
    ix.on_post_tool_use(&sess(), "Bash", "toolu_nested", "null", SUBAGENT_PATH)
        .await
        .unwrap();

    assert!(events.is_empty(), "got {events:?}");
    assert_eq!(stored_transcript_path(&ix).await.as_deref(), Some(OLD_PATH));
    let g = ix.store().inner.lock().unwrap();
    assert!(
        g.permissions
            .iter()
            .all(|r| r.tool_use_id.as_deref() != Some("toolu_nested")),
        "a subagent's tool call records no permission row on the parent"
    );
}

/// After the move the session's own `PreToolUse` / `PostToolUse` hooks name
/// the new path. They used to be taken for a subagent's and dropped; now they
/// are processed: the request is recorded, and a foreground subagent's window
/// opens and closes.
#[tokio::test]
async fn after_relocation_the_sessions_own_tool_hooks_are_processed() {
    let ix = interactor();
    session_with_one_exchange(&ix).await;
    ix.transcript_fake().relocate(OLD_PATH, NEW_PATH, NEW_CWD);

    ix.on_pre_tool_use(
        &sess(),
        "Bash",
        r#"{"command":"ls"}"#,
        "toolu_bash",
        NEW_PATH,
    )
    .await
    .unwrap();
    assert!(
        ix.store()
            .inner
            .lock()
            .unwrap()
            .permissions
            .iter()
            .any(|r| r.tool_use_id.as_deref() == Some("toolu_bash")),
        "the session's own PreToolUse records its request"
    );

    ix.transcript_fake().push_to(
        NEW_PATH,
        foreground_agent_tool_use_line("a-agent", "toolu_agent", "Agent", "general-purpose", "d"),
    );
    let started = ix
        .on_pre_tool_use(
            &sess(),
            "Agent",
            r#"{"subagent_type":"general-purpose","description":"d","prompt":"p","run_in_background":false}"#,
            "toolu_agent",
            NEW_PATH,
        )
        .await
        .unwrap();
    assert!(
        started
            .iter()
            .any(|e| matches!(e, SessionEvent::SubagentStarted { .. })),
        "got {started:?}"
    );
    let finished = ix
        .on_post_tool_use(&sess(), "Agent", "toolu_agent", "null", NEW_PATH)
        .await
        .unwrap();
    assert!(
        finished
            .iter()
            .any(|e| matches!(e, SessionEvent::SubagentFinished { .. })),
        "got {finished:?}"
    );
}

/// A path outside the transcript root is refused exactly as at registration:
/// it is never stored, and the hook is ignored.
#[tokio::test]
async fn a_relocation_outside_the_transcript_root_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let old_path = root.path().join("-work/sess-1.jsonl");
    let old_path = old_path.to_str().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().join("sess-1.jsonl");
    let outside_path = outside_path.to_str().unwrap();

    let ix = interactor_with_transcript_root(root.path().to_str().unwrap());
    ix.on_user_prompt_submit(submit_in("sess-1", old_path, "/work", "seed"))
        .await
        .unwrap();

    ix.on_pre_tool_use(
        &sess(),
        "Bash",
        r#"{"command":"ls"}"#,
        "toolu_outside",
        outside_path,
    )
    .await
    .unwrap();

    assert_eq!(stored_transcript_path(&ix).await.as_deref(), Some(old_path));
    assert!(ix
        .store()
        .inner
        .lock()
        .unwrap()
        .permissions
        .iter()
        .all(|r| r.tool_use_id.as_deref() != Some("toolu_outside")));
}
