use delta_model::SessionId;

use crate::interactor::testing::*;

/// The navigator lists sessions most-recently-active first. The sort key is a
/// session's last activity (`MAX(message.created_at)`), falling back to its own
/// `created_at` when it has no messages — so a message-less session sorts above
/// one whose only activity is older than that fallback.
#[tokio::test]
async fn list_sessions_orders_by_most_recent_activity() {
    let ix = interactor();

    // Three sessions registered in id order; all share the same `created_at`
    // (the fake store stamps a fixed registration time).
    ix.on_user_prompt_submit(submit_for("sess-old", "/tmp/old.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-new", "/tmp/new.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-quiet", "/tmp/quiet.jsonl", "seed"))
        .await
        .unwrap();
    // Open each session so the tail (scoped to open sessions) ingests their
    // seeded transcript growth below; the listing sort itself reads the store
    // regardless of open/closed.
    ix.bind_open_session("delta-old", &SessionId::from("sess-old"))
        .await;
    ix.bind_open_session("delta-new", &SessionId::from("sess-new"))
        .await;
    ix.bind_open_session("delta-quiet", &SessionId::from("sess-quiet"))
        .await;

    // `sess-old` last spoke before the shared registration time; `sess-new`
    // spoke after it. `sess-quiet` has no messages, so it falls back to its
    // `created_at` (the shared registration time, "2026-01-01T00:00:00Z").
    ix.transcript_fake().push_to(
        "/tmp/old.jsonl",
        assistant_line_at("a-old", "older", "2025-12-31T00:00:00Z"),
    );
    ix.transcript_fake().push_to(
        "/tmp/new.jsonl",
        assistant_line_at("a-new", "newer", "2026-02-01T00:00:00Z"),
    );
    ix.poll_transcript().await.unwrap();

    let ids: Vec<_> = ix
        .list_sessions()
        .await
        .unwrap()
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect();
    assert_eq!(
        ids,
        vec!["sess-new", "sess-quiet", "sess-old"],
        "most recent activity first; a message-less session sorts on its \
         created_at fallback, above one whose only activity is older"
    );
}
