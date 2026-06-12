use delta_model::{ContentBlock, Message, MessageUuid, SendStatus, Role, SessionId};
use delta_usecase::{NewSession, SessionPageCursor, SessionStore};

use super::SqliteStore;

fn new_session() -> NewSession {
    NewSession {
        id: "sess-1".into(),
        cwd: "/work".into(),
        transcript_path: "/tmp/t.jsonl".into(),
    }
}

fn new_session_with(id: &str) -> NewSession {
    NewSession {
        id: id.into(),
        cwd: "/work".into(),
        transcript_path: format!("/tmp/{id}.jsonl"),
    }
}

#[tokio::test]
async fn list_sessions_returns_all_in_deterministic_base_order() {
    let store = SqliteStore::open_in_memory().unwrap();
    store
        .register_session(new_session_with("sess-1"))
        .await
        .unwrap();
    store
        .register_session(new_session_with("sess-2"))
        .await
        .unwrap();

    // The store returns every registered session in a deterministic base order
    // (`created_at`, then `id` to break equal-timestamp ties). The navigator's
    // most-recently-active-first ordering is layered on in the usecase, which
    // also knows each session's last activity.
    let sessions = store.list_sessions().await.unwrap();
    let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["sess-1", "sess-2"]);

    // No sessions yet on a fresh store.
    let empty = SqliteStore::open_in_memory().unwrap();
    assert!(empty.list_sessions().await.unwrap().is_empty());
}

#[tokio::test]
async fn session_looks_up_by_id() {
    let store = SqliteStore::open_in_memory().unwrap();
    store
        .register_session(new_session_with("sess-1"))
        .await
        .unwrap();

    let found = store
        .session(&SessionId::from("sess-1"))
        .await
        .unwrap()
        .expect("registered session is found by id");
    assert_eq!(found.id.as_str(), "sess-1");
    assert_eq!(found.transcript_path, "/tmp/sess-1.jsonl");

    // An unknown id resolves to None.
    assert!(store
        .session(&SessionId::from("nope"))
        .await
        .unwrap()
        .is_none());
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
async fn dispatched_send_fifo_and_match() {
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

    let head = store.head_dispatched_send(&session.id).await.unwrap().unwrap();
    assert_eq!(head.id, first.id, "FIFO returns the oldest");
    assert_eq!(head.locator_quote.as_deref(), Some("[q]"));

    store
        .mark_send_matched(first.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();

    let head = store.head_dispatched_send(&session.id).await.unwrap().unwrap();
    assert_eq!(head.text, "second", "matched send leaves the queue");
}

#[tokio::test]
async fn match_dispatched_send_finds_oldest_pending_by_trimmed_text() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // Two dispatched sends with the same trimmed text; the oldest must win.
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
        .match_dispatched_send(&session.id, "hello world")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(matched.id, first.id, "returns the oldest matching dispatched send");

    // Marking it matched removes it from the candidate set.
    store
        .mark_send_matched(first.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();
    let matched = store
        .match_dispatched_send(&session.id, "hello world")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(matched.id, _second.id, "skips the already-matched send");

    // No dispatched send for a foreign text.
    assert!(store
        .match_dispatched_send(&session.id, "nope")
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
        created_at: Some("2026-01-01T00:00:00Z".into()),
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
async fn recent_workdirs_returns_distinct_cwds_in_recency_order() {
    let store = SqliteStore::open_in_memory().unwrap();

    let session_in = |id: &str, cwd: &str| NewSession {
        id: id.into(),
        cwd: cwd.into(),
        transcript_path: format!("/tmp/{id}.jsonl"),
    };

    // Three sessions across two distinct cwds. `/projects/b` is used by two
    // sessions; `/projects/a` by one. Recency is driven by message activity.
    let (a, a_main) = store
        .register_session(session_in("sess-a", "/projects/a"))
        .await
        .unwrap();
    let (b1, b1_main) = store
        .register_session(session_in("sess-b1", "/projects/b"))
        .await
        .unwrap();
    let (b2, b2_main) = store
        .register_session(session_in("sess-b2", "/projects/b"))
        .await
        .unwrap();

    let msg = |session_id: &SessionId, thread, uuid: &str, created_at: &str| Message {
        uuid: MessageUuid::from(uuid),
        session_id: session_id.clone(),
        thread_id: thread,
        role: Role::User,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq: 0,
        content_text: Some("hi".into()),
        content: vec![ContentBlock::Text { text: "hi".into() }],
        created_at: Some(created_at.into()),
    };

    // `/projects/a` had its latest activity at 00:10; `/projects/b`'s most
    // recent session (b2) had activity at 00:05. So `/projects/a` is more recent
    // even though `/projects/b` has more sessions.
    store
        .upsert_messages(&[
            msg(&a.id, a_main, "a-1", "2026-01-01T00:10:00Z"),
            msg(&b1.id, b1_main, "b1-1", "2026-01-01T00:01:00Z"),
            msg(&b2.id, b2_main, "b2-1", "2026-01-01T00:05:00Z"),
        ])
        .await
        .unwrap();

    let recent = store.recent_workdirs(10).await.unwrap();
    let paths: Vec<&str> = recent.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        vec!["/projects/a", "/projects/b"],
        "distinct cwds, most-recently-active first"
    );
    // Each cwd carries the max recency across its sessions.
    assert_eq!(recent[0].1.as_deref(), Some("2026-01-01T00:10:00Z"));
    assert_eq!(recent[1].1.as_deref(), Some("2026-01-01T00:05:00Z"));

    // The limit caps the result count.
    let one = store.recent_workdirs(1).await.unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].0, "/projects/a");
}

#[tokio::test]
async fn recent_workdirs_falls_back_to_created_at_for_message_less_sessions() {
    let store = SqliteStore::open_in_memory().unwrap();
    // A session with no messages still contributes its cwd, keyed by its own
    // `created_at`, so a freshly-used directory is listed before any message
    // lands.
    let (_s, _main) = store
        .register_session(NewSession {
            id: "sess-1".into(),
            cwd: "/fresh".into(),
            transcript_path: "/tmp/s.jsonl".into(),
        })
        .await
        .unwrap();

    let recent = store.recent_workdirs(10).await.unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].0, "/fresh");
    assert!(
        recent[0].1.is_some(),
        "recency falls back to the session's created_at"
    );
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
    // mirroring `match_dispatched_send` attaching it on its first (pending) hit.
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
    };
    store.upsert_messages(&[msg]).await.unwrap();

    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].created_at, None);
    // A timestamp-less message contributes no activity (MAX skips NULL).
    assert_eq!(store.last_activity_at(&session.id).await.unwrap(), None);
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
        }])
        .await
        .unwrap();
    let fetched = store.thread(child.id).await.unwrap().unwrap();
    assert_eq!(fetched.root_message_uuid, Some(root));
}

/// Register a session and stamp one message at `activity_at`, so its recency
/// (last activity) is fully controlled regardless of wall-clock registration
/// time. Returns the session id for assertions.
async fn session_active_at(store: &SqliteStore, id: &str, activity_at: &str) -> SessionId {
    let (session, main) = store.register_session(new_session_with(id)).await.unwrap();
    store
        .upsert_messages(&[Message {
            uuid: MessageUuid::from(format!("{id}-msg")),
            session_id: session.id.clone(),
            thread_id: main,
            role: Role::User,
            linear_parent_uuid: None,
            semantic_parent_uuid: None,
            prompt_id: None,
            seq: 0,
            content_text: Some("hi".into()),
            content: vec![ContentBlock::Text { text: "hi".into() }],
            created_at: Some(activity_at.into()),
        }])
        .await
        .unwrap();
    session.id
}

fn page_ids(rows: &[(delta_model::Session, Option<String>)]) -> Vec<String> {
    rows.iter().map(|(s, _)| s.id.as_str().to_owned()).collect()
}

#[tokio::test]
async fn list_sessions_page_orders_by_recency_descending() {
    let store = SqliteStore::open_in_memory().unwrap();
    session_active_at(&store, "sess-mid", "2026-02-01T00:00:00Z").await;
    session_active_at(&store, "sess-new", "2026-03-01T00:00:00Z").await;
    session_active_at(&store, "sess-old", "2026-01-01T00:00:00Z").await;

    let page = store.list_sessions_page(None, 10).await.unwrap();
    assert_eq!(page_ids(&page), vec!["sess-new", "sess-mid", "sess-old"]);
    // Each row carries its inline last activity; no follow-up lookup needed.
    assert_eq!(page[0].1.as_deref(), Some("2026-03-01T00:00:00Z"));
}

#[tokio::test]
async fn list_sessions_page_advances_across_pages_without_gap_or_overlap() {
    let store = SqliteStore::open_in_memory().unwrap();
    session_active_at(&store, "sess-a", "2026-04-01T00:00:00Z").await;
    session_active_at(&store, "sess-b", "2026-03-01T00:00:00Z").await;
    session_active_at(&store, "sess-c", "2026-02-01T00:00:00Z").await;
    session_active_at(&store, "sess-d", "2026-01-01T00:00:00Z").await;

    // First page of two, then resume after its last row.
    let first = store.list_sessions_page(None, 2).await.unwrap();
    assert_eq!(page_ids(&first), vec!["sess-a", "sess-b"]);

    let (last_session, last_activity) = first.last().unwrap();
    let cursor = SessionPageCursor {
        recency: last_activity.clone().unwrap(),
        created_at: last_session.created_at.clone(),
        id: last_session.id.as_str().to_owned(),
    };
    let second = store
        .list_sessions_page(Some(cursor), 2)
        .await
        .unwrap();
    assert_eq!(
        page_ids(&second),
        vec!["sess-c", "sess-d"],
        "the next page resumes strictly after the cursor with no gap or overlap"
    );
}

#[tokio::test]
async fn list_sessions_page_breaks_recency_ties_by_id_descending() {
    let store = SqliteStore::open_in_memory().unwrap();
    // Equal recency (and registration bursts tie `created_at` too, at second
    // resolution): the `id` tiebreaker must put the larger id first, because
    // Delta-minted ids are time-ordered UUID v7 — the newest session of a tie
    // still sorts first.
    let shared = "2026-01-01T00:00:00Z";
    session_active_at(&store, "sess-a", shared).await;
    session_active_at(&store, "sess-b", shared).await;

    let page = store.list_sessions_page(None, 10).await.unwrap();
    assert_eq!(page_ids(&page), vec!["sess-b", "sess-a"]);
}

#[tokio::test]
async fn list_sessions_page_falls_back_to_created_at_for_message_less_session() {
    let store = SqliteStore::open_in_memory().unwrap();
    // One active session whose activity is far in the past, plus a message-less
    // session. The message-less one falls back to its own (just-now) created_at,
    // which sorts above the old activity.
    session_active_at(&store, "sess-old", "2020-01-01T00:00:00Z").await;
    let (quiet, _) = store
        .register_session(new_session_with("sess-quiet"))
        .await
        .unwrap();

    let page = store.list_sessions_page(None, 10).await.unwrap();
    assert_eq!(
        page_ids(&page),
        vec!["sess-quiet", "sess-old"],
        "a message-less session sorts on its created_at fallback"
    );
    // The message-less row exposes a NULL last_activity_at (not the fallback).
    let quiet_row = page.iter().find(|(s, _)| s.id == quiet.id).unwrap();
    assert_eq!(quiet_row.1, None);
}

#[tokio::test]
async fn list_sessions_page_signals_more_via_full_page_only() {
    let store = SqliteStore::open_in_memory().unwrap();
    session_active_at(&store, "sess-a", "2026-02-01T00:00:00Z").await;
    session_active_at(&store, "sess-b", "2026-01-01T00:00:00Z").await;

    // A full page (returned count == limit) signals more may follow; the store
    // returns exactly `limit` rows. A short/last page returns fewer.
    let full = store.list_sessions_page(None, 2).await.unwrap();
    assert_eq!(full.len(), 2, "a full page returns exactly the limit");

    let short = store.list_sessions_page(None, 10).await.unwrap();
    assert_eq!(short.len(), 2, "a last page returns fewer than the limit");
}

#[tokio::test]
async fn permission_request_is_recorded() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    let req = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, "toolu_01")
        .await
        .unwrap();
    assert_eq!(req.tool_name, "Bash");
    assert_eq!(req.tool_use_id.as_deref(), Some("toolu_01"));
    assert!(req.id > 0);
}

#[tokio::test]
async fn find_open_permission_request_prefers_exact_input_then_latest() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();

    // Two pending Bash requests with different inputs.
    let ls = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, "toolu_01")
        .await
        .unwrap();
    let pwd = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"pwd"}"#, "toolu_02")
        .await
        .unwrap();

    // An exact tool_input match wins over the latest row.
    assert_eq!(
        store
            .find_open_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#)
            .await
            .unwrap(),
        Some(ls.id),
    );

    // Without an exact match, the most recent pending row for the tool wins.
    assert_eq!(
        store
            .find_open_permission_request(&session.id, "Bash", r#"{"command":"echo"}"#)
            .await
            .unwrap(),
        Some(pwd.id),
    );

    // Resolved rows are not candidates; once both are decided, nothing matches.
    store
        .resolve_permission_by_tool_use_id(&session.id, "toolu_01", true)
        .await
        .unwrap();
    store
        .resolve_permission_by_tool_use_id(&session.id, "toolu_02", true)
        .await
        .unwrap();
    assert_eq!(
        store
            .find_open_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#)
            .await
            .unwrap(),
        None,
    );

    // A different tool name does not match either.
    let _ = store
        .record_permission_request(&session.id, "Read", r#"{"path":"/a"}"#, "toolu_03")
        .await
        .unwrap();
    assert_eq!(
        store
            .find_open_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#)
            .await
            .unwrap(),
        None,
    );
}

#[tokio::test]
async fn permission_request_resolves_by_tool_use_id() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    let req = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, "toolu_01")
        .await
        .unwrap();

    // A non-matching tool_use_id resolves nothing.
    assert_eq!(
        store
            .resolve_permission_by_tool_use_id(&session.id, "toolu_other", true)
            .await
            .unwrap(),
        None,
    );

    // The matching, still-pending request resolves and returns its id.
    assert_eq!(
        store
            .resolve_permission_by_tool_use_id(&session.id, "toolu_01", true)
            .await
            .unwrap(),
        Some(req.id),
    );

    // A second resolve is a no-op: the request is no longer pending.
    assert_eq!(
        store
            .resolve_permission_by_tool_use_id(&session.id, "toolu_01", true)
            .await
            .unwrap(),
        None,
    );
}

#[tokio::test]
async fn turn_active_flag_defaults_false_and_round_trips() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _main) = store.register_session(new_session()).await.unwrap();

    assert!(
        !store.is_turn_active(&session.id).await.unwrap(),
        "a fresh session is idle"
    );

    store.set_turn_active(&session.id, true).await.unwrap();
    assert!(store.is_turn_active(&session.id).await.unwrap());

    store.set_turn_active(&session.id, false).await.unwrap();
    assert!(!store.is_turn_active(&session.id).await.unwrap());
}

#[tokio::test]
async fn queued_send_is_held_then_promoted_to_dispatched() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // A queued send is recorded but stays out of the dispatched FIFO and the
    // text-match candidate set until it is promoted.
    let queued = store
        .enqueue_queued_send(&session.id, main, None, "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(queued.status, SendStatus::Queued);
    assert!(
        store.head_dispatched_send(&session.id).await.unwrap().is_none(),
        "a queued send is not a dispatched FIFO head"
    );
    assert!(
        store
            .match_dispatched_send(&session.id, "branch text")
            .await
            .unwrap()
            .is_none(),
        "a queued send is not matchable until promoted"
    );

    let next = store
        .next_queued_send(&session.id)
        .await
        .unwrap()
        .expect("the queued send is the next to dispatch");
    assert_eq!(next.id, queued.id);

    // Promotion flips it to dispatched, so it now correlates as an ordinary send.
    store.promote_queued_send(queued.id).await.unwrap();
    assert!(
        store.next_queued_send(&session.id).await.unwrap().is_none(),
        "no queued sends remain after promotion"
    );
    let matched = store
        .match_dispatched_send(&session.id, "branch text")
        .await
        .unwrap()
        .expect("the promoted send is now matchable");
    assert_eq!(matched.id, queued.id);
    assert_eq!(matched.status, SendStatus::Dispatched);
    assert_eq!(matched.locator_quote.as_deref(), Some("quote"));
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
