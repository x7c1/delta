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
    // consumes the send and starts its turn).
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the re-typed send stays Dispatched until a prompt submission consumes it"
    );
}

/// Only the awaited send is stuck. `Dispatched` is not on its own a claim
/// that nobody heard about a send — a send consumed by a rewritten echo also
/// keeps that status until the turn ends — so the helper reads the turn
/// machine and re-types the one send it is waiting for, never every row it
/// finds. Pinned against a second `Dispatched` row, which the
/// single-outstanding rule makes rare but which recovery must not re-deliver.
#[tokio::test]
async fn session_start_compact_redispatches_only_the_awaited_send() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The awaited send: dispatched through the normal path, so the turn
    // machine is in `AwaitingEcho` for it and one set of keystrokes was sent.
    let awaited = ix
        .enqueue_send(to(main), "the awaited prompt", None)
        .await
        .unwrap()
        .0;
    assert_eq!(ix.tmux_fake().sent.lock().unwrap().len(), 1);

    // A second `Dispatched` row seeded straight through the store — the
    // dispatch path only ever promotes one at a time, so this is the only way
    // to put a row the turn machine knows nothing about in front of the
    // helper. `SessionStore::enqueue_send` records it already `Dispatched`
    // (the store-level seam the actor calls after typing the keystrokes).
    let bystander = ix
        .store()
        .enqueue_send(&session, main, None, "someone else's row", None)
        .await
        .unwrap();
    let dispatched = ix.store().dispatched_sends(&session).await.unwrap();
    assert_eq!(dispatched.len(), 2, "two dispatched rows face the helper");

    let _ = ix
        .on_session_start(session_start(session.as_str(), "compact"))
        .await
        .unwrap();

    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(
        sent.len(),
        2,
        "exactly one re-type on top of the original dispatch, got {sent:?}"
    );
    assert_eq!(
        sent[1].1.as_str(),
        "the awaited prompt",
        "the re-typed send is the one the turn machine awaits"
    );
    // Statuses unchanged: the re-type is keystrokes only.
    assert_eq!(
        ix.store().send(awaited.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched
    );
    assert_eq!(
        ix.store().send(bystander.id).await.unwrap().unwrap().status,
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
