use delta_model::{ContentBlock, Message, MessageUuid, Role};
use delta_usecase::{NewSession, SessionStore};

use super::SqliteStore;

fn new_session() -> NewSession {
    NewSession {
        id: "sess-1".into(),
        cwd: "/work".into(),
        transcript_path: "/tmp/t.jsonl".into(),
    }
}

#[tokio::test]
async fn register_is_idempotent_and_creates_main_thread() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    assert_eq!(session.id.as_str(), "sess-1");

    // Re-registering returns the same main thread, not a duplicate.
    let (_, main2) = store.register_session(new_session()).await.unwrap();
    assert_eq!(main, main2);
    assert_eq!(store.main_thread_id(&session.id).await.unwrap(), main);
}

#[tokio::test]
async fn pending_send_fifo_and_match() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let first = store
        .enqueue_send(&session.id, main, None, "first", Some("[q]"))
        .await
        .unwrap();
    let _second = store
        .enqueue_send(&session.id, main, None, "second", None)
        .await
        .unwrap();

    let head = store.head_pending_send(&session.id).await.unwrap().unwrap();
    assert_eq!(head.id, first.id, "FIFO returns the oldest");
    assert_eq!(head.locator_quote.as_deref(), Some("[q]"));

    store
        .mark_send_matched(first.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();

    let head = store.head_pending_send(&session.id).await.unwrap().unwrap();
    assert_eq!(head.text, "second", "matched send leaves the queue");
}

#[tokio::test]
async fn match_pending_send_finds_oldest_pending_by_trimmed_text() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // Two pending sends with the same trimmed text; the oldest must win.
    let first = store
        .enqueue_send(&session.id, main, None, "  hello world\n", None)
        .await
        .unwrap();
    let _second = store
        .enqueue_send(&session.id, main, None, "hello world", None)
        .await
        .unwrap();
    let _other = store
        .enqueue_send(&session.id, main, None, "different", None)
        .await
        .unwrap();

    // Trimmed comparison ignores surrounding whitespace on the stored text.
    let matched = store
        .match_pending_send(&session.id, "hello world")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(matched.id, first.id, "returns the oldest matching pending");

    // Marking it matched removes it from the candidate set.
    store
        .mark_send_matched(first.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();
    let matched = store
        .match_pending_send(&session.id, "hello world")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(matched.id, _second.id, "skips the already-matched send");

    // No pending send for a foreign text.
    assert!(store
        .match_pending_send(&session.id, "nope")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn message_upsert_and_thread_view() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let msg = Message {
        uuid: MessageUuid::from("u-1"),
        session_id: session.id.clone(),
        thread_id: main,
        role: Role::User,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq: 0,
        content_text: Some("hello".into()),
        content: vec![ContentBlock::Text {
            text: "hello".into(),
        }],
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    store
        .upsert_messages(std::slice::from_ref(&msg))
        .await
        .unwrap();
    assert_eq!(store.message_count(&session.id).await.unwrap(), 1);

    // Upsert same uuid with new content updates rather than duplicating.
    let mut updated = msg.clone();
    updated.content_text = Some("hello again".into());
    updated.content = vec![ContentBlock::Text {
        text: "hello again".into(),
    }];
    store.upsert_messages(&[updated]).await.unwrap();
    assert_eq!(store.message_count(&session.id).await.unwrap(), 1);

    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].content_text.as_deref(), Some("hello again"));
    assert_eq!(view[0].content.len(), 1);
}

#[tokio::test]
async fn transcript_lines_read_defaults_to_zero_and_persists_updates() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _main) = store.register_session(new_session()).await.unwrap();

    // A freshly registered session starts with an empty line cursor.
    assert_eq!(store.transcript_lines_read(&session.id).await.unwrap(), 0);

    store
        .set_transcript_lines_read(&session.id, 7)
        .await
        .unwrap();
    assert_eq!(store.transcript_lines_read(&session.id).await.unwrap(), 7);

    // Re-registering must not reset the cursor (INSERT OR IGNORE).
    store.register_session(new_session()).await.unwrap();
    assert_eq!(store.transcript_lines_read(&session.id).await.unwrap(), 7);
}

#[tokio::test]
async fn upsert_fills_missing_created_at() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // A transcript line without a timestamp arrives with an empty `created_at`.
    let msg = Message {
        uuid: MessageUuid::from("u-no-ts"),
        session_id: session.id.clone(),
        thread_id: main,
        role: Role::User,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq: 0,
        content_text: Some("hello".into()),
        content: vec![ContentBlock::Text {
            text: "hello".into(),
        }],
        created_at: String::new(),
    };
    store.upsert_messages(&[msg]).await.unwrap();

    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view.len(), 1);
    // The contract promises an ISO-8601 timestamp, never an empty string.
    let stored = &view[0].created_at;
    assert!(!stored.is_empty(), "created_at must be filled in");
    assert!(
        stored.ends_with('Z') && stored.contains('T'),
        "created_at must be ISO-8601, got {stored:?}"
    );
}

#[tokio::test]
async fn branch_thread_records_parent_and_root() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    let root = MessageUuid::from("u-root");
    let child = store
        .create_thread(&session.id, "branch", Some(main), Some(&root))
        .await
        .unwrap();
    let fetched = store.thread(child.id).await.unwrap().unwrap();
    assert_eq!(fetched.parent_thread_id, Some(main));
    assert_eq!(fetched.root_message_uuid, Some(root));
}

#[tokio::test]
async fn permission_request_is_recorded() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    let req = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#)
        .await
        .unwrap();
    assert_eq!(req.tool_name, "Bash");
    assert!(req.id > 0);
}
