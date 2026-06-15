use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// Each distinct session id registers independently on its first
/// `UserPromptSubmit`. A second submit for an already-registered id does not
/// re-register, but a submit for a new id does — registration is "first contact
/// for THIS id", not "first ever".
#[tokio::test]
async fn submit_registers_each_session_id_independently() {
    let ix = interactor();

    // First contact for sess-1 registers it.
    let (events1, _) = ix
        .on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "hi"))
        .await
        .unwrap();
    assert!(events1.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-1"),
    }));

    // A second submit for sess-1 must NOT re-register it.
    let (events1b, _) = ix
        .on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "again"))
        .await
        .unwrap();
    assert!(
        !events1b
            .iter()
            .any(|e| matches!(e, SessionEvent::SessionRegistered { .. })),
        "an already-registered id must not re-register"
    );

    // First contact for a DIFFERENT id registers that one too.
    let (events2, _) = ix
        .on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "hi"))
        .await
        .unwrap();
    assert!(events2.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-2"),
    }));

    // Both sessions now exist (membership, sorted so the assertion does not
    // depend on the page's recency ordering).
    let mut ids: Vec<String> = ix
        .list_sessions_page(None, 30)
        .await
        .unwrap()
        .listings
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect();
    ids.sort();
    assert_eq!(ids, vec!["sess-1".to_owned(), "sess-2".to_owned()]);
}
