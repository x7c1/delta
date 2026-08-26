//! A send that was consumed but never attributed settles as **delivered** when
//! its turn ends — `matched` with no uuid, never `cancelled`.
//!
//! The row has to leave `dispatched` at turn end or it would shadow the next
//! send's correlation. Cancelling was the old sweep, and it was wrong in the
//! one case it actually fires: the message was typed, a prompt submission
//! consumed it, and Claude answered it — recording that as "cancelled" tells
//! the user their delivered message failed. `matched_uuid` stays `NULL`: the
//! delivery is recorded, the transcript line is not claimed.
//!
//! An ingested human line consumes the send by position, so the row reaching
//! turn end unclaimed means the turn produced no human line at all: the prompt
//! was swallowed (a compaction routine), or the turn ended before its line was
//! flushed and ingested. That is the path pinned here — no user line is ever
//! pushed for the first send.

use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::StopHook;
use crate::turn::TurnState;

#[tokio::test]
async fn turn_end_settles_a_consumed_send_as_delivered() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix.enqueue_send(to(main), "run it", None).await.unwrap();
    // The prompt submits — consuming the send by position — but no user line is
    // ever written for it, so nothing claims the row.
    ix.on_user_prompt_submit(submit("run it (v2)"))
        .await
        .unwrap();
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "mid-turn the row waits for a transcript line that never arrives",
    );

    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    let settled = ix.store().send(send.id).await.unwrap().unwrap();
    assert_eq!(
        settled.status,
        SendStatus::Matched,
        "delivered messages settle as matched, not cancelled",
    );
    assert_eq!(settled.matched_uuid, None, "no transcript line was claimed");
    assert_eq!(ix.live_state_for(&session).await.turn, TurnState::Idle);

    // Nothing stale is left behind: the next send correlates normally.
    let (next, _) = ix.enqueue_send(to(main), "and again", None).await.unwrap();
    assert_eq!(next.status, SendStatus::Dispatched);
    assert_eq!(
        ix.store()
            .head_dispatched_send(&session)
            .await
            .unwrap()
            .map(|s| s.id),
        Some(next.id),
        "the settled row no longer shadows the outstanding send",
    );
    ix.transcript_fake().push(user_line("u-2", "and again"));
    ix.on_user_prompt_submit(submit("and again")).await.unwrap();
    let matched = ix.store().send(next.id).await.unwrap().unwrap();
    assert_eq!(matched.status, SendStatus::Matched);
    assert_eq!(
        matched.matched_uuid.as_ref().map(|u| u.as_str()),
        Some("u-2"),
        "a send whose text does match still gets its uuid",
    );
}
