use delta_model::SessionId;

use crate::interactor::testing::*;

/// The background tail polls only *open* (live-pane) sessions, never every
/// session in the store.
///
/// Regression for the external-resume leak: a session that is closed in Delta
/// (no live pane) can still grow its shared on-disk JSONL when it is resumed
/// from an external terminal (`claude --resume <id>` outside Delta). The tail
/// must not ingest that growth or stream it into the UI for a session Delta
/// holds no pane for. With one open and one closed session both having
/// un-ingested transcript growth, only the open one is ingested and returned;
/// the closed one is left untouched.
#[tokio::test]
async fn poll_transcript_only_polls_open_sessions() {
    let ix = interactor();

    // An open session: registered and bound to a live pane.
    ix.on_user_prompt_submit(submit_for("sess-open", "/tmp/open.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-open", &SessionId::from("sess-open"))
        .await;

    // A closed session: registered (a known data session) but never bound, so it
    // has no live pane.
    ix.on_user_prompt_submit(submit_for("sess-closed", "/tmp/closed.jsonl", "seed"))
        .await
        .unwrap();
    let closed = SessionId::from("sess-closed");
    assert!(
        !ix.is_session_open(&closed).await,
        "precondition: the second session is closed (no live pane)",
    );
    let closed_main = ix.store().main_thread_id(&closed).await.unwrap();

    // Both transcripts grow with an un-ingested assistant line. For the closed
    // session this stands in for an external `claude --resume` writing to the
    // shared JSONL.
    ix.transcript_fake()
        .push_to("/tmp/open.jsonl", assistant_line("a-open", "open reply"));
    ix.transcript_fake().push_to(
        "/tmp/closed.jsonl",
        assistant_line("a-closed", "external resume reply"),
    );

    let (groups, _events) = ix.poll_transcript().await.unwrap();

    // Only the open session is ingested and returned.
    assert_eq!(groups.len(), 1, "only the open session is polled");
    assert_eq!(
        groups[0][0].session_id.as_str(),
        "sess-open",
        "the returned group is the open session's",
    );
    let open_uuids: Vec<&str> = groups[0].iter().map(|m| m.uuid.as_str()).collect();
    assert_eq!(open_uuids, vec!["a-open"], "the open session's late line");

    // The closed session is untouched: its external growth was never ingested.
    let closed_view = ix.thread_view(closed_main).await.unwrap();
    assert!(
        closed_view.iter().all(|m| m.uuid.as_str() != "a-closed"),
        "the closed session's external growth must not leak into the store",
    );
}
