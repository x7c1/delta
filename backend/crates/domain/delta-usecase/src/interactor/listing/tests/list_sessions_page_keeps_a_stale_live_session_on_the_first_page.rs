use delta_model::SessionId;

use super::listed_ids;
use crate::interactor::testing::*;

/// A live session whose recency would put it past the first `limit` rows is
/// still on the first page — `limit` bounds the closed portion, not the whole
/// page — and walking the cursor chain to `None` still lists every session
/// exactly once: no gap where the live rows were skipped, no duplicate of them.
#[tokio::test]
async fn list_sessions_page_keeps_a_stale_live_session_on_the_first_page() {
    let ix = interactor();

    let seed = [
        ("sess-a", "/tmp/a.jsonl", "2026-04-01T00:00:00Z"),
        ("sess-b", "/tmp/b.jsonl", "2026-03-01T00:00:00Z"),
        ("sess-c", "/tmp/c.jsonl", "2026-02-01T00:00:00Z"),
        ("sess-live", "/tmp/live.jsonl", "2026-01-01T00:00:00Z"),
    ];
    for (id, transcript, at) in seed {
        ix.on_user_prompt_submit(submit_for(id, transcript, "seed"))
            .await
            .unwrap();
        // Bound so the transcript tail (scoped to open sessions) ingests the
        // seeded activity that gives each session its distinct recency.
        ix.bind_open_session(&format!("delta-{id}"), &SessionId::from(id))
            .await;
        ix.transcript_fake()
            .push_to(transcript, assistant_line_at(id, "seed", at));
    }
    ix.poll_transcript().await.unwrap();
    // Everything but `sess-live` closes; `sess-live` is the least recently
    // active, so recency alone would bury it on the last page.
    for id in ["sess-a", "sess-b", "sess-c"] {
        ix.close_session(&SessionId::from(id)).await.unwrap();
    }

    // Page one: the live session, then a full `limit` of closed ones.
    let first = ix.list_sessions_page(None, 2).await.unwrap();
    let first_ids = listed_ids(&first);
    assert_eq!(first_ids, vec!["sess-live", "sess-a", "sess-b"]);
    assert!(
        first.next.is_some(),
        "a full closed portion yields a cursor"
    );

    // Page two resumes after `sess-b` in the closed stream. `sess-live` is
    // filtered out of it (already listed), so the page is short and terminal.
    let second = ix.list_sessions_page(first.next, 2).await.unwrap();
    assert_eq!(listed_ids(&second), vec!["sess-c"]);
    assert!(
        second.next.is_none(),
        "a short closed portion ends the chain"
    );

    let mut seen: Vec<String> = first_ids;
    seen.extend(listed_ids(&second));
    seen.sort();
    assert_eq!(
        seen,
        vec!["sess-a", "sess-b", "sess-c", "sess-live"],
        "the walk covers every session exactly once"
    );
}
