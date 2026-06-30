use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;

/// Ingesting a JSONL fragment whose last line is an `isCompactSummary:true`
/// record (i.e. Claude Code's auto-`/compact` finished writing its summary)
/// while a `Dispatched` send is outstanding must re-type that send to the
/// pane exactly once. This is the ingestion-path companion of the
/// `SessionStart(source=compact)` hook: on cold-start replay no live hook
/// fires, so the `Role::CompactSummary` ingest is the only signal.
#[tokio::test]
async fn compact_summary_redispatches_stuck_dispatched() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A send into an idle, open session dispatches immediately. After this
    // the row is `Dispatched` and exactly one set of keystrokes has been
    // sent (the original dispatch).
    let (send, _) = ix
        .enqueue_send(to(main), "the user's actual prompt", None)
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    let original_dispatch_count = ix.tmux_fake().sent.lock().unwrap().len();
    assert_eq!(original_dispatch_count, 1);

    // Claude writes the compaction summary line; the background tail ingests
    // it. The pure attribution fold classifies it as `Role::CompactSummary`
    // and emits `Effect::AutoCompactFinished`; the sync interactor turns the
    // effect into a re-type via the existing `TmuxDriver::send_line` path.
    ix.transcript_fake().push(compact_summary_line(
        "cs-1",
        "<summary>of the previous conversation</summary>",
    ));
    let (_groups, _events) = ix.poll_transcript().await.unwrap();

    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(
        sent.len(),
        original_dispatch_count + 1,
        "the compact-summary ingest re-typed the stuck send exactly once, got {sent:?}"
    );
    assert_eq!(
        sent.last().unwrap().1.as_str(),
        "the user's actual prompt",
        "the re-type carried the stuck send's own text"
    );
    // The send row is still `Dispatched` — only the keystrokes were re-sent.
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the re-typed send stays Dispatched until the echo matches it"
    );
}

/// Idempotency at the ingest layer: when the same compact event drives both
/// the live `SessionStart(source=compact)` hook AND the ingestion-time
/// `Effect::AutoCompactFinished` within the debounce window, re-dispatch
/// must fire exactly once. Asserted by firing the hook first (which claims
/// the debounce) and then ingesting the compact summary line; the ingest
/// must observe the debounce and not re-type.
#[tokio::test]
async fn compact_summary_redispatch_is_debounced_against_hook() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (_send, _) = ix
        .enqueue_send(to(main), "the user's actual prompt", None)
        .await
        .unwrap();
    let baseline = ix.tmux_fake().sent.lock().unwrap().len();
    assert_eq!(baseline, 1);

    // The hook fires first and re-types the stuck send.
    ix.on_session_start(session_start(session.as_str(), "compact"))
        .await
        .unwrap();
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        baseline + 1,
        "hook re-typed the send"
    );

    // The summary line then lands in the transcript; the ingest must observe
    // the debounce claimed by the hook and skip its own re-type.
    ix.transcript_fake().push(compact_summary_line(
        "cs-1",
        "<summary>of the previous conversation</summary>",
    ));
    ix.poll_transcript().await.unwrap();
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        baseline + 1,
        "ingest within the debounce window must not re-type a second time"
    );
}
