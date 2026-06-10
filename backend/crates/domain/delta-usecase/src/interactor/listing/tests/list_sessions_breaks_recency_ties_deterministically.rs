use crate::interactor::testing::*;

/// Equal recency keys order deterministically by the `created_at` then `id`
/// tiebreaker, so the list never reshuffles between calls for sessions with the
/// same last-activity timestamp.
#[tokio::test]
async fn list_sessions_breaks_recency_ties_deterministically() {
    let ix = interactor();

    // Two sessions, no messages: both fall back to the same (shared)
    // `created_at`, so only the `id` tiebreaker distinguishes them.
    ix.on_user_prompt_submit(submit_for("sess-b", "/tmp/b.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-a", "/tmp/a.jsonl", "seed"))
        .await
        .unwrap();

    let order = || async {
        ix.list_sessions()
            .await
            .unwrap()
            .iter()
            .map(|l| l.session.id.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    // Registered b-then-a, but the ascending `id` tiebreaker puts "sess-a"
    // first, and repeated calls agree.
    assert_eq!(order().await, vec!["sess-a", "sess-b"]);
    assert_eq!(order().await, vec!["sess-a", "sess-b"], "order is stable");
}
