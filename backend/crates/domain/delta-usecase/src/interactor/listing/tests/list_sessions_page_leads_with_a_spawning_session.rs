use delta_model::SessionId;

use super::listed_ids;
use crate::interactor::testing::*;

/// A spawn still in flight — accepted, its row written, but nothing bound to it
/// yet — belongs to the leading group. It is the session the user just started,
/// so it must not sit below every closed one and jump up a few seconds later
/// when its first hook binds it. It leads while still reporting `open: false`.
#[tokio::test]
async fn list_sessions_page_leads_with_a_spawning_session() {
    let ix = interactor();

    // A closed session that was active far more recently than the spawn's row.
    ix.on_user_prompt_submit(submit_for("sess-closed", "/tmp/closed.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-closed", &SessionId::from("sess-closed"))
        .await;
    ix.transcript_fake().push_to(
        "/tmp/closed.jsonl",
        assistant_line_at("a-closed", "newer", "2026-06-01T00:00:00Z"),
    );
    ix.poll_transcript().await.unwrap();
    ix.close_session(&SessionId::from("sess-closed"))
        .await
        .unwrap();

    // Accept a spawn; no hook has bound it, so it is `spawning`, not open.
    ix.new_session().await.unwrap();
    let spawning = ix.pending_session_ids().await.remove(0);

    let page = ix.list_sessions_page(None, 30).await.unwrap();
    assert_eq!(
        listed_ids(&page),
        vec![spawning.as_str().to_owned(), "sess-closed".to_owned()],
        "an in-flight spawn leads the closed session"
    );
    assert!(
        !page.listings[0].open,
        "the spawn is live for ordering but not yet open"
    );
    assert!(
        !page.listings[1].open,
        "the closed session is neither live nor open"
    );
}
