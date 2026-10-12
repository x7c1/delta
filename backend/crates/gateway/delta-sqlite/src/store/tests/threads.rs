//! Branch-thread derivation and the per-thread recency column.

use delta_model::{ContentBlock, Message, MessageUuid, Role, SessionId, ThreadId};

use super::super::SqliteStore;
use super::{new_session, new_session_with};

/// A minimal user message on `thread_id`, timestamped with `created_at`.
fn message(
    session_id: &SessionId,
    thread_id: ThreadId,
    uuid: &str,
    seq: i64,
    created_at: Option<&str>,
) -> Message {
    Message {
        uuid: MessageUuid::from(uuid),
        session_id: session_id.clone(),
        thread_id,
        role: Role::User,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq,
        content_text: Some("hi".into()),
        content: vec![ContentBlock::Text { text: "hi".into() }],
        created_at: created_at.map(str::to_owned),
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
        provider_item_id: None,
    }
}

#[tokio::test]
async fn branch_thread_derives_root_from_send_then_message() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    let root = MessageUuid::from("u-root");
    let child = store
        .create_thread(&session.id, "branch", Some(main))
        .await
        .unwrap();
    assert_eq!(child.parent_thread_id, Some(main));
    assert_eq!(
        child.root_message_uuid, None,
        "no branch send or message exists yet to derive the root from"
    );

    // Once the branch send is recorded, the thread's root is derived from it.
    store
        .enqueue_send(&session.id, child.id, Some(&root), "branch reply", None)
        .await
        .unwrap();
    let fetched = store.thread(child.id).await.unwrap().unwrap();
    assert_eq!(fetched.parent_thread_id, Some(main));
    assert_eq!(fetched.root_message_uuid, Some(root.clone()));

    // Once the branch message itself is ingested, it becomes the source.
    store
        .upsert_messages(&[Message {
            uuid: MessageUuid::from("u-branch-1"),
            session_id: session.id.clone(),
            thread_id: child.id,
            role: Role::User,
            linear_parent_uuid: None,
            semantic_parent_uuid: Some(root.clone()),
            prompt_id: None,
            seq: 0,
            content_text: Some("branch reply".into()),
            content: vec![ContentBlock::Text {
                text: "branch reply".into(),
            }],
            created_at: Some("2026-01-01T00:00:00Z".into()),
            model: None,
            git_branch: None,
            cwd: None,
            response_time_ms: None,
            provider_item_id: None,
        }])
        .await
        .unwrap();
    let fetched = store.thread(child.id).await.unwrap().unwrap();
    assert_eq!(fetched.root_message_uuid, Some(root));
}

#[tokio::test]
async fn upsert_messages_maintains_each_touched_threads_last_activity() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    let branch = store
        .create_thread(&session.id, "branch", Some(main))
        .await
        .unwrap();

    // A freshly created thread has no activity to report.
    assert_eq!(branch.last_activity_at, None);
    assert_eq!(
        store.thread(main).await.unwrap().unwrap().last_activity_at,
        None,
    );

    // One batch spanning both threads of the session: each thread must end up
    // with the newest timestamp of ITS OWN messages, not the batch-wide max.
    store
        .upsert_messages(&[
            message(&session.id, main, "m-1", 0, Some("2026-01-01T00:00:00Z")),
            message(&session.id, main, "m-2", 1, Some("2026-01-01T00:01:00Z")),
            message(
                &session.id,
                branch.id,
                "b-1",
                2,
                Some("2026-01-01T00:00:30Z"),
            ),
        ])
        .await
        .unwrap();

    let recency = |threads: &[delta_model::Thread], id: ThreadId| -> Option<String> {
        threads
            .iter()
            .find(|t| t.id == id)
            .expect("the thread is listed")
            .last_activity_at
            .clone()
    };
    let threads = store.list_threads(&session.id).await.unwrap();
    assert_eq!(
        recency(&threads, main).as_deref(),
        Some("2026-01-01T00:01:00Z"),
    );
    assert_eq!(
        recency(&threads, branch.id).as_deref(),
        Some("2026-01-01T00:00:30Z"),
        "the branch keeps its own newest message, not the session's",
    );

    // A later message on the branch moves only the branch.
    store
        .upsert_messages(&[message(
            &session.id,
            branch.id,
            "b-2",
            3,
            Some("2026-01-01T00:09:00Z"),
        )])
        .await
        .unwrap();
    let threads = store.list_threads(&session.id).await.unwrap();
    assert_eq!(
        recency(&threads, main).as_deref(),
        Some("2026-01-01T00:01:00Z"),
        "an untouched thread's recency is left alone",
    );
    assert_eq!(
        recency(&threads, branch.id).as_deref(),
        Some("2026-01-01T00:09:00Z"),
    );

    // Re-ingesting the very same batch is idempotent: the column is recomputed
    // as a MAX over the thread's messages, never accumulated.
    store
        .upsert_messages(&[
            message(&session.id, main, "m-1", 0, Some("2026-01-01T00:00:00Z")),
            message(&session.id, main, "m-2", 1, Some("2026-01-01T00:01:00Z")),
            message(
                &session.id,
                branch.id,
                "b-1",
                2,
                Some("2026-01-01T00:00:30Z"),
            ),
            message(
                &session.id,
                branch.id,
                "b-2",
                3,
                Some("2026-01-01T00:09:00Z"),
            ),
        ])
        .await
        .unwrap();
    let threads = store.list_threads(&session.id).await.unwrap();
    assert_eq!(
        recency(&threads, main).as_deref(),
        Some("2026-01-01T00:01:00Z"),
    );
    assert_eq!(
        recency(&threads, branch.id).as_deref(),
        Some("2026-01-01T00:09:00Z"),
    );
}

#[tokio::test]
async fn a_thread_whose_messages_carry_no_timestamp_keeps_its_recency_null() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // A transcript line without a timestamp contributes nothing: MAX over no
    // value is NULL, never a sentinel.
    store
        .upsert_messages(&[message(&session.id, main, "m-no-ts", 0, None)])
        .await
        .unwrap();

    assert_eq!(
        store.thread(main).await.unwrap().unwrap().last_activity_at,
        None,
    );
}

/// The batch read returns the threads of exactly the named sessions, ascending
/// id, so each session's rows match what `list_threads` returns for it — with
/// the derived `root_message_uuid` filled in the same way.
#[tokio::test]
async fn list_threads_by_session_ids_returns_each_named_sessions_threads() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (a, a_main) = store
        .register_session(new_session_with("sess-a"))
        .await
        .unwrap();
    let (b, b_main) = store
        .register_session(new_session_with("sess-b"))
        .await
        .unwrap();
    let (other, other_main) = store
        .register_session(new_session_with("sess-other"))
        .await
        .unwrap();
    // Interleave the branches across sessions so ascending id mixes them.
    let a_branch = store
        .create_thread(&a.id, "a branch", Some(a_main))
        .await
        .unwrap();
    store
        .create_thread(&other.id, "other branch", Some(other_main))
        .await
        .unwrap();
    store
        .create_thread(&b.id, "b branch", Some(b_main))
        .await
        .unwrap();
    store
        .enqueue_send(
            &a.id,
            a_branch.id,
            Some(&MessageUuid::from("u-root")),
            "x",
            None,
        )
        .await
        .unwrap();

    let batch = store
        .list_threads_by_session_ids(&[a.id.clone(), b.id.clone()])
        .await
        .unwrap();
    let mut expected = store.list_threads(&a.id).await.unwrap();
    expected.extend(store.list_threads(&b.id).await.unwrap());
    expected.sort_by_key(|t| t.id);
    assert_eq!(batch, expected);
    assert!(
        batch.iter().all(|t| t.session_id != other.id),
        "an unnamed session contributes nothing"
    );
    assert_eq!(
        batch
            .iter()
            .find(|t| t.id == a_branch.id)
            .unwrap()
            .root_message_uuid,
        Some(MessageUuid::from("u-root")),
    );

    assert!(store
        .list_threads_by_session_ids(&[])
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .list_threads_by_session_ids(&[SessionId::from("ghost")])
        .await
        .unwrap()
        .is_empty());
}
