use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionStore;
use crate::turn::TurnState;

/// Each live row carries its session's turn, read through the same live-state
/// query the sends envelope uses: a turn in flight is listed `InFlight` on the
/// thread `in_progress_turn_thread` resolves, so the list, `/sends` and the
/// turn events name the same thread. An idle live session and a closed one are
/// both listed idle with no thread.
#[tokio::test]
async fn list_sessions_page_reports_the_turn_in_flight() {
    let ix = interactor();
    ix.seed_session().await;
    let running = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&running).await.unwrap();

    // A second session, live and idle (its seeding turn stopped).
    ix.on_user_prompt_submit(submit_for("sess-idle", "/tmp/idle.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_stop(crate::ports::StopHook {
        session_id: SessionId::from("sess-idle"),
        stop_reason: None,
    })
    .await
    .unwrap();
    ix.bind_open_session("delta-idle", &SessionId::from("sess-idle"))
        .await;

    // A third session, closed.
    ix.on_user_prompt_submit(submit_for("sess-closed", "/tmp/closed.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-closed", &SessionId::from("sess-closed"))
        .await;
    ix.close_session(&SessionId::from("sess-closed"))
        .await
        .unwrap();

    // Start a turn on the first session from a dispatched send.
    let (send, _) = ix.enqueue_send(to(main), "hello", None).await.unwrap();
    ix.on_user_prompt_submit(submit("hello")).await.unwrap();
    let expected_thread = ix.store().in_progress_turn_thread(&running).await.unwrap();
    assert_eq!(expected_thread, send.thread_id);

    let page = ix.list_sessions_page(None, 30).await.unwrap();
    let row = |id: &str| {
        page.listings
            .iter()
            .find(|l| l.session.id.as_str() == id)
            .unwrap_or_else(|| panic!("{id} is paged"))
    };

    let in_flight = row("sess-1");
    assert_eq!(
        in_flight.turn,
        TurnState::InFlight {
            send_id: Some(send.id)
        }
    );
    assert_eq!(in_flight.in_progress_thread, Some(expected_thread));
    assert_eq!(
        (in_flight.turn, in_flight.in_progress_thread),
        {
            let live = ix.live_state_for(&running).await;
            (live.turn, live.in_progress_thread)
        },
        "the list reports the turn the sends envelope does"
    );

    for id in ["sess-idle", "sess-closed"] {
        let listing = row(id);
        assert_eq!(listing.turn, TurnState::Idle, "{id} is listed idle");
        assert_eq!(listing.in_progress_thread, None, "{id} names no thread");
        assert!(
            listing.running_subagents.is_empty(),
            "{id} runs no subagent"
        );
    }
    assert!(row("sess-idle").open, "the idle session is live");
    assert!(!row("sess-closed").open, "the closed session is not");
}
