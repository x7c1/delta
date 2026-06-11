use delta_model::SessionId;

use crate::interactor::testing::*;

/// `poll_transcript` syncs every *open* session and groups the new lines per
/// session, so the caller can announce each session's growth separately.
#[tokio::test]
async fn poll_transcript_groups_new_lines_per_session() {
    let ix = interactor();
    // Two open sessions: register each, then bind a live pane so the tail (which
    // is scoped to open sessions) polls them.
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-1", &SessionId::from("sess-1"))
        .await;
    ix.on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-2", &SessionId::from("sess-2"))
        .await;

    // Both sessions flush a late assistant line.
    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-1", "reply one"));
    ix.transcript_fake()
        .push_to("/tmp/s2.jsonl", assistant_line("a-2", "reply two"));

    let (groups, _events) = ix.poll_transcript().await.unwrap();
    assert_eq!(groups.len(), 2, "one group per session that grew");

    // Each group carries exactly its own session's new line.
    let mut by_session: Vec<(String, Vec<String>)> = groups
        .iter()
        .map(|g| {
            (
                g[0].session_id.as_str().to_owned(),
                g.iter().map(|m| m.uuid.as_str().to_owned()).collect(),
            )
        })
        .collect();
    by_session.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        by_session,
        vec![
            ("sess-1".to_owned(), vec!["a-1".to_owned()]),
            ("sess-2".to_owned(), vec!["a-2".to_owned()]),
        ]
    );

    // A second poll with no new lines yields no groups (per-session cursors
    // advanced independently).
    assert!(ix.poll_transcript().await.unwrap().0.is_empty());
}
