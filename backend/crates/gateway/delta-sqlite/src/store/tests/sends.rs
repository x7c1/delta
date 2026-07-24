//! The send FIFO transitions.

use delta_model::{MessageUuid, SendStatus};

use super::super::SqliteStore;
use super::{new_session, new_session_with};

#[tokio::test]
async fn dispatched_send_fifo_and_match() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let first = store
        .enqueue_send(&session.id, main, None, "first", Some("[q]"))
        .await
        .unwrap();
    let _second = store
        .enqueue_send(&session.id, main, None, "second", None)
        .await
        .unwrap();

    let head = store
        .head_dispatched_send(&session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(head.id, first.id, "FIFO returns the oldest");
    assert_eq!(head.locator_quote.as_deref(), Some("[q]"));

    store
        .mark_send_matched(first.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();

    let head = store
        .head_dispatched_send(&session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(head.text, "second", "matched send leaves the queue");
}

#[tokio::test]
async fn requeue_send_returns_a_dispatched_send_to_queued() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let send = store
        .enqueue_send(&session.id, main, None, "hello world", None)
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);

    // Requeue moves it out of the dispatched slot and back into the queue.
    store.requeue_send(send.id).await.unwrap();
    assert!(
        store
            .head_dispatched_send(&session.id)
            .await
            .unwrap()
            .is_none(),
        "a requeued send is no longer outstanding"
    );
    let next = store
        .next_queued_send(&session.id)
        .await
        .unwrap()
        .expect("the requeued send is the next to dispatch");
    assert_eq!(next.id, send.id);
    assert_eq!(next.status, SendStatus::Queued);

    // Requeue is dispatched-only: a matched send is terminal-for-correlation
    // and must not be pulled back into the queue.
    store.promote_queued_send(send.id).await.unwrap();
    store
        .mark_send_matched(send.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();
    store.requeue_send(send.id).await.unwrap();
    assert!(
        store.next_queued_send(&session.id).await.unwrap().is_none(),
        "a matched send is not requeued"
    );
}

#[tokio::test]
async fn restore_all_dispatched_sweeps_every_session_and_spares_other_rows() {
    // The boot-time reconcile: turn state is rebuilt Idle on boot, so every
    // persisted `dispatched` row is an orphan — the sweep returns each of
    // them to `queued` **with the restored marker set** across ALL sessions,
    // while terminal (`matched` / `cancelled`) rows and genuinely `queued`
    // rows stay untouched (the queued row must keep dispatching normally, so
    // it must not gain the marker).
    let store = SqliteStore::open_in_memory().unwrap();
    let (first, first_main) = store
        .register_session(new_session_with("sess-1"))
        .await
        .unwrap();
    let (second, second_main) = store
        .register_session(new_session_with("sess-2"))
        .await
        .unwrap();

    // One dispatched orphan per session (`enqueue_send` writes `dispatched`).
    let orphan_a = store
        .enqueue_send(&first.id, first_main, None, "orphan a", None)
        .await
        .unwrap();
    let orphan_b = store
        .enqueue_send(&second.id, second_main, None, "orphan b", None)
        .await
        .unwrap();

    // Rows the sweep must not touch: terminal ones and a genuinely queued one.
    let matched = store
        .enqueue_send(&first.id, first_main, None, "matched", None)
        .await
        .unwrap();
    store
        .mark_send_matched(matched.id, &MessageUuid::from("u-m"))
        .await
        .unwrap();
    let cancelled = store
        .enqueue_send(&second.id, second_main, None, "cancelled", None)
        .await
        .unwrap();
    store.cancel_send(cancelled.id).await.unwrap();
    let queued = store
        .enqueue_queued_send(&first.id, first_main, None, "still queued", None)
        .await
        .unwrap();

    let restored = store.restore_all_dispatched().await.unwrap();
    assert_eq!(restored, 2, "exactly the two dispatched orphans transition");

    for (id, session) in [(orphan_a.id, &first.id), (orphan_b.id, &second.id)] {
        let send = store.send(id).await.unwrap().unwrap();
        assert_eq!(send.status, SendStatus::Queued);
        assert!(
            send.restored_at.is_some(),
            "a restored row carries the marker so it awaits an explicit release"
        );
        assert!(
            store.head_dispatched_send(session).await.unwrap().is_none(),
            "no dispatched row survives the sweep"
        );
    }
    assert_eq!(
        store.send(matched.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
    );
    assert_eq!(
        store.send(cancelled.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
    );
    let queued = store.send(queued.id).await.unwrap().unwrap();
    assert_eq!(queued.status, SendStatus::Queued);
    assert_eq!(
        queued.restored_at, None,
        "a genuinely queued row is untouched — it keeps dispatching normally"
    );
}

#[tokio::test]
async fn next_queued_send_skips_restored_rows() {
    // A restored row must never dispatch automatically: `next_queued_send`
    // (the only selection every idle-dispatch trigger goes through) skips it
    // even when it is the oldest queued row, and picks a younger genuinely
    // queued row instead.
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let restored = store
        .enqueue_send(&session.id, main, None, "restored", None)
        .await
        .unwrap();
    assert_eq!(store.restore_all_dispatched().await.unwrap(), 1);

    // Only the restored row exists: nothing is selectable.
    assert!(
        store.next_queued_send(&session.id).await.unwrap().is_none(),
        "a restored row is not auto-dispatched"
    );
    // The restored row still shows in the open-send list (the UI needs it).
    let open = store.open_sends(&session.id).await.unwrap();
    assert_eq!(open.len(), 1);
    assert!(open[0].restored_at.is_some());

    // A younger genuinely queued row is selected past the older restored one.
    let fresh = store
        .enqueue_queued_send(&session.id, main, None, "fresh", None)
        .await
        .unwrap();
    let next = store
        .next_queued_send(&session.id)
        .await
        .unwrap()
        .expect("the unrestored queued row dispatches");
    assert_eq!(next.id, fresh.id);
    let _ = restored;
}

#[tokio::test]
async fn release_restored_send_clears_the_marker_only_for_queued_restored_rows() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let restored = store
        .enqueue_send(&session.id, main, None, "restored", None)
        .await
        .unwrap();
    assert_eq!(store.restore_all_dispatched().await.unwrap(), 1);

    // The release clears the marker; the row re-enters the normal queued flow.
    assert!(
        store.release_restored_send(restored.id).await.unwrap(),
        "a queued restored row releases"
    );
    let released = store.send(restored.id).await.unwrap().unwrap();
    assert_eq!(released.status, SendStatus::Queued);
    assert_eq!(released.restored_at, None);
    assert_eq!(
        store
            .next_queued_send(&session.id)
            .await
            .unwrap()
            .expect("a released row is selectable again")
            .id,
        restored.id,
    );

    // A second release is a no-op conflict: the row is already released.
    assert!(!store.release_restored_send(restored.id).await.unwrap());

    // A never-restored queued row is not releasable.
    let plain = store
        .enqueue_queued_send(&session.id, main, None, "plain", None)
        .await
        .unwrap();
    assert!(!store.release_restored_send(plain.id).await.unwrap());

    // A cancelled restored row is not releasable (the cancel won the race).
    let cancelled = store
        .enqueue_send(&session.id, main, None, "cancelled", None)
        .await
        .unwrap();
    assert_eq!(store.restore_all_dispatched().await.unwrap(), 1);
    assert!(store.cancel_queued_send(cancelled.id).await.unwrap());
    assert!(!store.release_restored_send(cancelled.id).await.unwrap());

    // An unknown id reports no transition rather than erroring.
    assert!(!store.release_restored_send(9999).await.unwrap());
}

#[tokio::test]
async fn queued_send_is_held_then_promoted_to_dispatched() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // A queued send is recorded but stays out of the outstanding (dispatched)
    // slot until it is promoted.
    let queued = store
        .enqueue_queued_send(&session.id, main, None, "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(queued.status, SendStatus::Queued);
    assert!(
        store
            .head_dispatched_send(&session.id)
            .await
            .unwrap()
            .is_none(),
        "a queued send is not a dispatched FIFO head"
    );

    let next = store
        .next_queued_send(&session.id)
        .await
        .unwrap()
        .expect("the queued send is the next to dispatch");
    assert_eq!(next.id, queued.id);

    // Promotion flips it to dispatched, so it now correlates as an ordinary send.
    store.promote_queued_send(queued.id).await.unwrap();
    assert!(
        store.next_queued_send(&session.id).await.unwrap().is_none(),
        "no queued sends remain after promotion"
    );
    let matched = store
        .head_dispatched_send(&session.id)
        .await
        .unwrap()
        .expect("the promoted send is now the outstanding dispatched send");
    assert_eq!(matched.id, queued.id);
    assert_eq!(matched.status, SendStatus::Dispatched);
    assert_eq!(matched.locator_quote.as_deref(), Some("quote"));
}

#[tokio::test]
async fn cancel_queued_send_only_cancels_while_queued() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // A queued send cancels: the guarded transition reports it moved, the row is
    // terminal (`cancelled`), and it drops out of both the queue and the
    // open-send list so the idle dispatch path never reaches it.
    let queued = store
        .enqueue_queued_send(&session.id, main, None, "held", None)
        .await
        .unwrap();
    assert!(
        store.cancel_queued_send(queued.id).await.unwrap(),
        "a queued send transitions to cancelled"
    );
    assert_eq!(
        store.send(queued.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
    );
    assert!(
        store.next_queued_send(&session.id).await.unwrap().is_none(),
        "a cancelled send is skipped by the idle dispatch path"
    );
    assert!(
        store.open_sends(&session.id).await.unwrap().is_empty(),
        "a cancelled send drops out of the open-send list"
    );
    // A second cancel is now a no-op: the row already left `queued`.
    assert!(
        !store.cancel_queued_send(queued.id).await.unwrap(),
        "re-cancelling an already-cancelled send reports no transition"
    );

    // A dispatched send is not cancellable through the guarded path: the row
    // stays dispatched and the transition reports no change.
    let dispatched = store
        .enqueue_send(&session.id, main, None, "typed", None)
        .await
        .unwrap();
    assert_eq!(dispatched.status, SendStatus::Dispatched);
    assert!(
        !store.cancel_queued_send(dispatched.id).await.unwrap(),
        "a dispatched send is not cancellable while dispatched"
    );
    assert_eq!(
        store.send(dispatched.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the dispatched row is left untouched"
    );

    // An unknown id reports no transition rather than erroring.
    assert!(!store.cancel_queued_send(9999).await.unwrap());
}

#[tokio::test]
async fn open_sends_lists_non_terminal_sends_oldest_first_per_session() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    let (other, other_main) = store
        .register_session(new_session_with("sess-2"))
        .await
        .unwrap();

    // Mix of statuses for the session under test: a dispatched send, a queued
    // send, a matched one, and a cancelled one. Only the first two are open.
    let dispatched = store
        .enqueue_send(&session.id, main, None, "dispatched", None)
        .await
        .unwrap();
    let queued = store
        .enqueue_queued_send(&session.id, main, None, "queued", None)
        .await
        .unwrap();
    let matched = store
        .enqueue_send(&session.id, main, None, "matched", None)
        .await
        .unwrap();
    store
        .mark_send_matched(matched.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();
    let cancelled = store
        .enqueue_send(&session.id, main, None, "cancelled", None)
        .await
        .unwrap();
    store.cancel_send(cancelled.id).await.unwrap();
    // A foreign session's open send must never leak into this session's list.
    store
        .enqueue_send(&other.id, other_main, None, "foreign", None)
        .await
        .unwrap();

    let open = store.open_sends(&session.id).await.unwrap();
    let ids: Vec<i64> = open.iter().map(|s| s.id).collect();
    assert_eq!(
        ids,
        vec![dispatched.id, queued.id],
        "only queued/dispatched sends, oldest first"
    );
    assert_eq!(open[0].status, SendStatus::Dispatched);
    assert_eq!(open[1].status, SendStatus::Queued);

    // A session with no open sends yields an empty list, not an error.
    store
        .mark_send_matched(dispatched.id, &MessageUuid::from("u-2"))
        .await
        .unwrap();
    store.cancel_send(queued.id).await.unwrap();
    assert!(store.open_sends(&session.id).await.unwrap().is_empty());
}
