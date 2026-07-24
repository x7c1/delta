//! Branch-thread derivation.

use delta_model::{ContentBlock, Message, MessageUuid, Role};

use super::super::SqliteStore;
use super::new_session;

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
