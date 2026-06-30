use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;

/// `SessionStart(source=compact)` fires once Claude Code finishes auto- or
/// manually compacting a live session. The compaction routine swallows any
/// prompt the user keyed in at the same moment (no `UserPromptSubmit` echo,
/// no `Stop`), so a `Dispatched` `OutstandingSend` is stuck behind a missing
/// echo. The hook must re-type each such send exactly once so the user's
/// intent is preserved.
#[tokio::test]
async fn session_start_compact_redispatches_stuck_dispatched() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A send into an idle, open session dispatches immediately: the row is
    // `Dispatched` and one set of keystrokes was sent (the seed turn fires no
    // keystrokes, so `sent` is exactly one entry).
    let (send, _) = ix
        .enqueue_send(to(main), "the user's actual prompt", None)
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "the seed dispatched the send exactly once before compaction"
    );

    // Auto-`/compact` finishes: the `SessionStart(source=compact)` hook fires.
    // The handler re-types the stuck send (status stays `Dispatched`).
    let events = ix
        .on_session_start(session_start(session.as_str(), "compact"))
        .await
        .unwrap();

    assert!(events.is_empty(), "compact emits no SessionEvents");
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(
        sent.len(),
        2,
        "compact re-typed the stuck send exactly once on top of the original \
         dispatch, got {sent:?}"
    );
    assert_eq!(
        sent[1].1.as_str(),
        "the user's actual prompt",
        "compact re-typed the stuck send's own text"
    );
    // The send row is still `Dispatched` — the re-type is just keystrokes,
    // it does not change the row's status (the next `UserPromptSubmit`
    // echo will resolve it via the normal `SendMatched` flow).
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the re-typed send stays Dispatched until the echo matches it"
    );
}

/// Two `Dispatched` sends for the same session must both be re-typed in
/// FIFO order on a single `compact` hook. Pins the helper's behaviour even
/// though the single-outstanding rule normally caps the queue at one — the
/// helper is the recovery path, so it must not silently drop later entries
/// if more than one is somehow outstanding.
#[tokio::test]
async fn session_start_compact_redispatches_all_dispatched_in_order() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Seed two `dispatched` send rows directly through the store: the
    // single-outstanding dispatch path normally only ever promotes one at a
    // time, so this is the only way to test the re-dispatch helper against
    // a multi-entry queue. `SessionStore::enqueue_send` records the row
    // already in `Dispatched` (this is the store-level seam the actor calls
    // after typing the keystrokes), so no extra promotion is needed. They
    // share the session, the thread, and arrive in id order (ascending) —
    // the FIFO the helper must preserve.
    let s1 = ix
        .store()
        .enqueue_send(&session, main, None, "first stuck prompt", None)
        .await
        .unwrap();
    let s2 = ix
        .store()
        .enqueue_send(&session, main, None, "second stuck prompt", None)
        .await
        .unwrap();
    // Sanity: store now reports two `Dispatched` rows.
    let dispatched = ix.store().dispatched_sends(&session).await.unwrap();
    assert_eq!(dispatched.len(), 2);

    // (The seed turn fires no keystrokes; the store-level seeding above
    // bypasses tmux entirely. So `sent` starts empty.)
    let before = ix.tmux_fake().sent.lock().unwrap().len();
    assert_eq!(before, 0);

    let _ = ix
        .on_session_start(session_start(session.as_str(), "compact"))
        .await
        .unwrap();

    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(
        sent.len(),
        2,
        "both dispatched sends are re-typed on one call, got {sent:?}"
    );
    assert_eq!(sent[0].1.as_str(), "first stuck prompt", "FIFO order");
    assert_eq!(sent[1].1.as_str(), "second stuck prompt", "FIFO order");
    // Statuses unchanged.
    assert_eq!(
        ix.store().send(s1.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched
    );
    assert_eq!(
        ix.store().send(s2.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched
    );
}

/// Idempotency: when both the live `SessionStart(source=compact)` hook and
/// the ingestion-time `Effect::AutoCompactFinished` fire for the same compact
/// event within the debounce window, re-dispatch must run exactly once.
/// Asserted by firing the hook back-to-back: the second call must see no
/// new keystrokes from the helper because the debounce stamp is fresh.
#[tokio::test]
async fn session_start_compact_redispatch_is_debounced() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (_send, _) = ix
        .enqueue_send(to(main), "the user's actual prompt", None)
        .await
        .unwrap();
    let baseline = ix.tmux_fake().sent.lock().unwrap().len();
    assert_eq!(baseline, 1, "one original dispatch before compact");

    // First compact: the stuck send is re-typed (the second tmux entry).
    ix.on_session_start(session_start(session.as_str(), "compact"))
        .await
        .unwrap();
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        baseline + 1,
        "first compact re-typed the send"
    );

    // Second compact, immediately after: the debounce stamp is still fresh,
    // so the helper is suppressed and no further keystrokes are sent.
    ix.on_session_start(session_start(session.as_str(), "compact"))
        .await
        .unwrap();
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        baseline + 1,
        "second compact within the debounce window must not re-type again"
    );
}
