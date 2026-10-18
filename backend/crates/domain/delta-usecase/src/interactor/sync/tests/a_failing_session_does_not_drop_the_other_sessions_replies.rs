use delta_model::SessionId;

use crate::interactor::testing::*;

/// One session whose background tick fails outside its transcript read (here,
/// its store lookup) is reported for itself — at `warn`, naming the session,
/// the stage, and the cause — and the replies of every other session in the
/// same tick are still returned.
#[tokio::test]
async fn a_failing_session_does_not_drop_the_other_sessions_replies() {
    let ix = interactor();
    for (session, pane, path) in [
        ("sess-1", "delta-1", "/tmp/s1.jsonl"),
        ("sess-2", "delta-2", "/tmp/s2.jsonl"),
        ("sess-3", "delta-3", "/tmp/s3.jsonl"),
    ] {
        ix.on_user_prompt_submit(submit_for(session, path, "seed"))
            .await
            .unwrap();
        ix.bind_open_session(pane, &SessionId::from(session)).await;
    }
    ix.transcript_fake()
        .push_to("/tmp/s2.jsonl", assistant_line("a-2", "reply two"));
    ix.transcript_fake()
        .push_to("/tmp/s3.jsonl", assistant_line("a-3", "reply three"));
    ix.store().inner.lock().unwrap().fail_session_lookup_for = Some(SessionId::from("sess-2"));
    let (logs, _guard) = capture_warnings();

    let (groups, _events) = ix.poll_transcript(TICK_BOUND).await;

    let mut ingested: Vec<_> = groups
        .iter()
        .flatten()
        .map(|m| (m.session_id.as_str(), m.uuid.as_str()))
        .collect();
    ingested.sort();
    assert_eq!(
        ingested,
        vec![("sess-3", "a-3")],
        "the failing session's error does not discard the others' replies"
    );
    let logged = logs.text();
    let line = logged
        .lines()
        .find(|l| l.contains("failed its background tick"))
        .unwrap_or_else(|| panic!("the failure is reported: {logged}"));
    assert!(
        line.contains("WARN")
            && line.contains("sess-2")
            && line.contains("transcript poll")
            && line.contains("injected session lookup failure"),
        "the warning names the session, the stage and the cause: {line}"
    );
}
