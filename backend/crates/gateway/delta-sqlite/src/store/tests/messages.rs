//! Message upserts, metadata round trips, the sync cursor, and FTS.

use delta_model::{ContentBlock, Message, MessageUuid, Role, SessionId};

use super::super::SqliteStore;
use super::new_session;

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
        created_at: Some("2026-01-01T00:00:00Z".into()),
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
        provider_item_id: None,
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

/// A `compact_summary` role must persist: the attribution fold produces it for
/// the synthetic line Claude Code writes on `/compact`, and the STRICT `message`
/// table's role CHECK has to accept it (it previously omitted the value, so a
/// real `/compact` write would fail the constraint — the fake store used by the
/// usecase tests hid it).
#[tokio::test]
async fn compact_summary_role_persists_and_round_trips() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let msg = Message {
        uuid: MessageUuid::from("cs-1"),
        session_id: session.id.clone(),
        thread_id: main,
        role: Role::CompactSummary,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq: 0,
        content_text: Some("previous conversation summary".into()),
        content: vec![ContentBlock::Text {
            text: "previous conversation summary".into(),
        }],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
        provider_item_id: None,
    };
    store
        .upsert_messages(std::slice::from_ref(&msg))
        .await
        .unwrap();

    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].role, Role::CompactSummary);
}

#[tokio::test]
async fn message_metadata_round_trips_through_upsert_and_read() {
    // The per-message metadata columns (model, git_branch, cwd, response_time_ms)
    // must survive the INSERT and read back into the right domain fields. This
    // guards the column ordering in `MESSAGE_COLS`, the INSERT/ON CONFLICT bind
    // list, and `message_from_row` against an off-by-one that would silently
    // swap or drop a field. A re-upsert of the same uuid with different metadata
    // must refresh it (it is transcript-derived cache, not overlay).
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let msg = Message {
        uuid: MessageUuid::from("a-1"),
        session_id: session.id.clone(),
        thread_id: main,
        role: Role::Assistant,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq: 0,
        content_text: Some("answer".into()),
        content: vec![ContentBlock::Text {
            text: "answer".into(),
        }],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        model: Some("claude-opus-4-8".into()),
        git_branch: Some("feature/meta".into()),
        cwd: Some("/home/dev/repo".into()),
        response_time_ms: Some(9400.5),
        provider_item_id: None,
    };
    store
        .upsert_messages(std::slice::from_ref(&msg))
        .await
        .unwrap();

    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].model.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(view[0].git_branch.as_deref(), Some("feature/meta"));
    assert_eq!(view[0].cwd.as_deref(), Some("/home/dev/repo"));
    assert_eq!(view[0].response_time_ms, Some(9400.5));

    // A re-ingest with changed metadata refreshes the cached columns.
    let mut updated = msg.clone();
    updated.model = Some("claude-sonnet-4-8".into());
    updated.git_branch = None;
    updated.response_time_ms = Some(1200.0);
    store.upsert_messages(&[updated]).await.unwrap();

    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].model.as_deref(), Some("claude-sonnet-4-8"));
    assert_eq!(view[0].git_branch, None, "a metadata value can be cleared");
    assert_eq!(view[0].cwd.as_deref(), Some("/home/dev/repo"));
    assert_eq!(view[0].response_time_ms, Some(1200.0));
}

#[tokio::test]
async fn last_activity_at_returns_latest_message_timestamp() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // No messages yet: no activity timestamp.
    assert_eq!(store.last_activity_at(&session.id).await.unwrap(), None);

    let make = |uuid: &str, seq: i64, created_at: &str| Message {
        uuid: MessageUuid::from(uuid),
        session_id: session.id.clone(),
        thread_id: main,
        role: Role::User,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq,
        content_text: Some("hi".into()),
        content: vec![ContentBlock::Text { text: "hi".into() }],
        created_at: Some(created_at.into()),
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
        provider_item_id: None,
    };
    store
        .upsert_messages(&[
            make("u-1", 0, "2026-01-01T00:00:00Z"),
            make("u-2", 1, "2026-01-01T00:05:00Z"),
        ])
        .await
        .unwrap();

    assert_eq!(
        store.last_activity_at(&session.id).await.unwrap(),
        Some("2026-01-01T00:05:00Z".to_string()),
    );
}

#[tokio::test]
async fn last_activity_at_is_stored_on_session_and_recomputed_on_reingest() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let make = |uuid: &str, seq: i64, created_at: Option<&str>| Message {
        uuid: MessageUuid::from(uuid),
        session_id: session.id.clone(),
        thread_id: main,
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
    };

    // The recency lives on the `session` row as a denormalized column, written
    // by the upsert — not derived from a per-row scan of `message`. Read it
    // straight from `session` to prove it is physically stored there.
    store
        .upsert_messages(&[
            make("u-1", 0, Some("2026-01-01T00:00:00Z")),
            make("u-2", 1, Some("2026-01-01T00:05:00Z")),
        ])
        .await
        .unwrap();
    assert_eq!(
        stored_last_activity(&store, &session.id).await.as_deref(),
        Some("2026-01-01T00:05:00Z"),
    );

    // A re-ingest that *lowers* the latest message's timestamp must pull the
    // stored recency back down: it is recomputed as the MAX over the session's
    // messages, not a monotonic high-water mark.
    store
        .upsert_messages(&[make("u-2", 1, Some("2026-01-01T00:02:00Z"))])
        .await
        .unwrap();
    assert_eq!(
        stored_last_activity(&store, &session.id).await.as_deref(),
        Some("2026-01-01T00:02:00Z"),
    );

    // A message with no timestamp contributes nothing: the stored recency stays
    // NULL (MAX over no value).
    let fresh = SqliteStore::open_in_memory().unwrap();
    let (s2, m2) = fresh.register_session(new_session()).await.unwrap();
    fresh
        .upsert_messages(&[Message {
            session_id: s2.id.clone(),
            thread_id: m2,
            ..make("u-x", 0, None)
        }])
        .await
        .unwrap();
    assert_eq!(
        stored_last_activity(&fresh, &s2.id).await,
        None,
        "a timestamp-less message leaves recency NULL",
    );
}

/// Read `session.last_activity_at` straight from the row, bypassing the
/// accessor, so a test can prove the value is physically denormalized onto the
/// session rather than derived on read.
async fn stored_last_activity(store: &SqliteStore, id: &SessionId) -> Option<String> {
    let conn = store.conn.lock().await;
    conn.query_row(
        "SELECT last_activity_at FROM session WHERE id = ?1",
        rusqlite::params![id.as_str()],
        |r| r.get::<_, Option<String>>(0),
    )
    .unwrap()
}

#[tokio::test]
async fn upsert_preserves_thread_overlay_on_reingest() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let branch = store
        .create_thread(&session.id, "branch", Some(main))
        .await
        .unwrap();
    let semantic_parent = MessageUuid::from("u-root");

    // First ingest: the line is correctly attributed to the branch thread,
    // mirroring the outstanding-send correlation attaching it on its first hit.
    let msg = Message {
        uuid: MessageUuid::from("u-1"),
        session_id: session.id.clone(),
        thread_id: branch.id,
        role: Role::User,
        linear_parent_uuid: None,
        semantic_parent_uuid: Some(semantic_parent.clone()),
        prompt_id: None,
        seq: 0,
        content_text: Some("hello".into()),
        content: vec![ContentBlock::Text {
            text: "hello".into(),
        }],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
        provider_item_id: None,
    };
    store
        .upsert_messages(std::slice::from_ref(&msg))
        .await
        .unwrap();

    // Second ingest of the SAME uuid: a re-sync fell back to main (the pending
    // is now `matched`, so it can only recompute `(main, None)`) but carries
    // refreshed content.
    let reingest = Message {
        thread_id: main,
        semantic_parent_uuid: None,
        content_text: Some("hello again".into()),
        content: vec![ContentBlock::Text {
            text: "hello again".into(),
        }],
        ..msg.clone()
    };
    store.upsert_messages(&[reingest]).await.unwrap();

    // The overlay (thread_id + semantic_parent_uuid) survives the re-ingest, so
    // the message stays on the branch thread...
    let branch_view = store.thread_messages(branch.id).await.unwrap();
    assert_eq!(branch_view.len(), 1, "message stays on the branch thread");
    assert_eq!(branch_view[0].thread_id, branch.id);
    assert_eq!(
        branch_view[0].semantic_parent_uuid.as_ref(),
        Some(&semantic_parent),
        "semantic parent overlay is preserved"
    );
    // ...and was NOT clobbered back to main.
    assert!(
        store.thread_messages(main).await.unwrap().is_empty(),
        "re-ingest must not move the message back to main"
    );
    // Content columns still refresh on conflict.
    assert_eq!(
        branch_view[0].content_text.as_deref(),
        Some("hello again"),
        "content columns still update on conflict"
    );
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
async fn upsert_keeps_missing_created_at_null() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // A transcript line without a timestamp stores NULL — never a sentinel
    // value — and round-trips back as `None`.
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
        created_at: None,
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
        provider_item_id: None,
    };
    store.upsert_messages(&[msg]).await.unwrap();

    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].created_at, None);
    // A timestamp-less message contributes no activity (MAX skips NULL).
    assert_eq!(store.last_activity_at(&session.id).await.unwrap(), None);
}

/// All `message_fts` rowids matching `query`, via the trigger-maintained index.
async fn fts_hits(store: &SqliteStore, query: &str) -> Vec<i64> {
    let conn = store.conn.lock().await;
    let mut stmt = conn
        .prepare("SELECT rowid FROM message_fts WHERE message_fts MATCH ?1")
        .unwrap();
    let rows = stmt.query_map([query], |r| r.get(0)).unwrap();
    rows.map(Result::unwrap).collect()
}

#[tokio::test]
async fn message_fts_indexes_inserts_and_updates() {
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
        content_text: Some("the quick brown fox".into()),
        content: vec![ContentBlock::Text {
            text: "the quick brown fox".into(),
        }],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
        provider_item_id: None,
    };
    store
        .upsert_messages(std::slice::from_ref(&msg))
        .await
        .unwrap();
    assert_eq!(fts_hits(&store, "quick").await.len(), 1);

    // A re-ingest with refreshed content replaces the indexed text rather than
    // duplicating or stranding the old entry.
    let mut updated = msg;
    updated.content_text = Some("a lazy dog".into());
    updated.content = vec![ContentBlock::Text {
        text: "a lazy dog".into(),
    }];
    store.upsert_messages(&[updated]).await.unwrap();
    assert!(fts_hits(&store, "quick").await.is_empty());
    assert_eq!(fts_hits(&store, "lazy").await.len(), 1);
}
