use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::TranscriptRefusal;

/// One session's refused transcript is settled inside its own actor: the poll
/// still succeeds, and another session's new lines are ingested on the same
/// tick.
#[tokio::test]
async fn a_refused_transcript_does_not_stop_another_session_from_ingesting() {
    let ix = interactor();
    for (session, pane, path) in [
        ("sess-1", "delta-1", "/tmp/s1.jsonl"),
        ("sess-2", "delta-2", "/tmp/s2.jsonl"),
    ] {
        ix.on_user_prompt_submit(submit_for(session, path, "seed"))
            .await
            .unwrap();
        ix.bind_open_session(pane, &SessionId::from(session)).await;
    }

    ix.transcript_fake()
        .refuse("/tmp/s1.jsonl", TranscriptRefusal::NotRegularFile);
    ix.transcript_fake()
        .push_to("/tmp/s2.jsonl", assistant_line("a-2", "reply two"));

    let (groups, _events) = ix.poll_transcript(TICK_BOUND).await.unwrap();
    let ingested: Vec<_> = groups
        .iter()
        .flatten()
        .map(|m| (m.session_id.as_str(), m.uuid.as_str()))
        .collect();
    assert_eq!(ingested, vec![("sess-2", "a-2")]);
}
