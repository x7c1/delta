use delta_model::{SessionId, SessionStatus};

use super::listed_ids;
use crate::interactor::testing::*;

/// A launch that never bound stays in the list, and sits exactly where any
/// other non-live session does.
///
/// Its row is kept and marked `failed`, so the user can open it, read why it
/// did not start, retry it, or remove it. Nothing about it is live — no pane,
/// nothing to bind — so it does not belong to the leading group the list puts
/// live sessions in; it joins the closed ones and is ordered among them by
/// recency, which for a session that ingested nothing is its own `created_at`.
#[tokio::test]
async fn list_sessions_page_lists_a_failed_launch_among_the_closed_ones() {
    let ix = interactor();

    // A closed session whose last activity predates everything else here.
    ix.on_user_prompt_submit(submit_for("sess-older", "/tmp/older.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-older", &SessionId::from("sess-older"))
        .await;
    ix.transcript_fake().push_to(
        "/tmp/older.jsonl",
        assistant_line_at("a-older", "older", "2025-06-01T00:00:00Z"),
    );
    ix.poll_transcript(TICK_BOUND).await.unwrap();
    ix.close_session(&SessionId::from("sess-older"))
        .await
        .unwrap();

    // A session that is open right now: the live head.
    ix.on_user_prompt_submit(submit_for("sess-live", "/tmp/live.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-live", &SessionId::from("sess-live"))
        .await;

    // And a launch that ends without ever binding: accepted, its row written,
    // then closed while still starting, which cancels the launch.
    ix.new_session().await.unwrap();
    let failed = ix.pending_session_ids().await.remove(0);
    ix.close_session(&failed).await.unwrap();
    assert_eq!(
        ix.store()
            .session(&failed)
            .await
            .unwrap()
            .expect("the failed launch keeps its row")
            .status,
        SessionStatus::Failed,
    );

    let page = ix.list_sessions_page(None, 30).await.unwrap();
    assert_eq!(
        listed_ids(&page),
        vec![
            "sess-live".to_owned(),
            failed.as_str().to_owned(),
            "sess-older".to_owned(),
        ],
        "the failed launch is listed: behind the live head, and among the \
         closed sessions in recency order"
    );
    assert!(
        !page.listings[1].open,
        "a failed launch has nothing bound to it, so it is not open"
    );
}
