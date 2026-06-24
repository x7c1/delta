use delta_model::{
    ContentBlock, Message, MessageUuid, PermissionStatus, Role, SendStatus, SessionId,
    SessionStatus, ThreadId,
};
use delta_usecase::{NewSession, SessionPageCursor, SessionStore};

use super::SqliteStore;

fn new_session() -> NewSession {
    NewSession {
        id: "sess-1".into(),
        cwd: "/work".into(),
        transcript_path: "/tmp/t.jsonl".into(),
        branch_at_launch: None,
        repo_root: None,
    }
}

fn new_session_with(id: &str) -> NewSession {
    NewSession {
        id: id.into(),
        cwd: "/work".into(),
        transcript_path: format!("/tmp/{id}.jsonl"),
        branch_at_launch: None,
        repo_root: None,
    }
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
    assert_eq!(found.transcript_path.as_deref(), Some("/tmp/sess-1.jsonl"));

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
async fn requeue_send_returns_a_dispatched_send_to_queued() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    let send = store
        .enqueue_send(&session.id, main, None, "hello world", None)
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);

    // Requeue moves it out of the dispatched slot and back into the queue.
    store.requeue_send(send.id).await.unwrap();
    assert!(
        store.head_dispatched_send(&session.id).await.unwrap().is_none(),
        "a requeued send is no longer outstanding"
    );
    let next = store
        .next_queued_send(&session.id)
        .await
        .unwrap()
        .expect("the requeued send is the next to dispatch");
    assert_eq!(next.id, send.id);
    assert_eq!(next.status, SendStatus::Queued);

    // Requeue is dispatched-only: a matched send is terminal-for-correlation
    // and must not be pulled back into the queue.
    store.promote_queued_send(send.id).await.unwrap();
    store
        .mark_send_matched(send.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();
    store.requeue_send(send.id).await.unwrap();
    assert!(
        store.next_queued_send(&session.id).await.unwrap().is_none(),
        "a matched send is not requeued"
    );
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
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
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

/// An existing database created before `session.last_activity_at` existed must
/// gain the column and be backfilled on open, without losing data.
#[tokio::test]
async fn opening_a_pre_column_database_migrates_and_backfills() {
    let dir = std::env::temp_dir().join(format!("delta-migrate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legacy.sqlite");
    let path_str = path.to_str().unwrap();

    // Build a "legacy" database the way it shipped *before* the column: open it
    // through the store (which now adds `last_activity_at`), then physically
    // drop the column so the file looks pre-migration. This keeps every other
    // table identical to the real old schema rather than hand-rolling a partial
    // copy. SQLite's `DROP COLUMN` also removes the dependent expression index,
    // so the file is a faithful pre-`last_activity_at` snapshot.
    {
        let legacy = SqliteStore::open(path_str).unwrap();
        legacy
            .register_session(NewSession {
                id: "with-msgs".into(),
                cwd: "/w".into(),
                transcript_path: "/tmp/with.jsonl".into(),
                branch_at_launch: None,
                repo_root: None,
            })
            .await
            .unwrap();
        let (no_msgs, _) = legacy
            .register_session(NewSession {
                id: "no-msgs".into(),
                cwd: "/w".into(),
                transcript_path: "/tmp/no.jsonl".into(),
                branch_at_launch: None,
                repo_root: None,
            })
            .await
            .unwrap();
        let main = legacy
            .main_thread_id(&SessionId::from("with-msgs"))
            .await
            .unwrap();
        let msg = |uuid: &str, at: &str| Message {
            uuid: MessageUuid::from(uuid),
            session_id: SessionId::from("with-msgs"),
            thread_id: main,
            role: Role::User,
            linear_parent_uuid: None,
            semantic_parent_uuid: None,
            prompt_id: None,
            seq: 0,
            content_text: Some("hi".into()),
            content: vec![ContentBlock::Text { text: "hi".into() }],
            created_at: Some(at.into()),
            model: None,
            git_branch: None,
            cwd: None,
            response_time_ms: None,
        };
        legacy
            .upsert_messages(&[
                msg("m1", "2026-01-05T00:00:00Z"),
                msg("m2", "2026-01-09T00:00:00Z"),
            ])
            .await
            .unwrap();
        let _ = no_msgs;
        let conn = legacy.conn.lock().await;
        // Strip the column so the file is a faithful pre-migration snapshot to
        // re-open. The expression index references the column, so drop it first.
        conn.execute_batch(
            "DROP INDEX ix_session_recency; \
             ALTER TABLE session DROP COLUMN last_activity_at;",
        )
        .unwrap();
    }

    // Opening through the store applies the guarded ALTER + backfill.
    let store = SqliteStore::open(path_str).unwrap();

    // The message-bearing session is backfilled to its MAX(message.created_at).
    assert_eq!(
        store
            .last_activity_at(&SessionId::from("with-msgs"))
            .await
            .unwrap()
            .as_deref(),
        Some("2026-01-09T00:00:00Z"),
    );
    // The message-less session stays NULL (the navigator orders it on its own
    // created_at).
    assert_eq!(
        store
            .last_activity_at(&SessionId::from("no-msgs"))
            .await
            .unwrap(),
        None,
    );

    // Re-opening the now-migrated database is a clean no-op (the column already
    // exists, so the guarded ALTER does not run again).
    drop(store);
    let reopened = SqliteStore::open(path_str).unwrap();
    assert_eq!(
        reopened
            .last_activity_at(&SessionId::from("with-msgs"))
            .await
            .unwrap()
            .as_deref(),
        Some("2026-01-09T00:00:00Z"),
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// A database created before the per-message metadata columns
/// (`model`/`git_branch`/`cwd`/`response_time_ms`) must gain them on open and
/// load its pre-existing rows with those fields as NULL — never crashing on the
/// now-wider `message_from_row` and never losing the row's other data.
#[tokio::test]
async fn opening_a_pre_metadata_database_migrates_and_loads_old_rows_as_null() {
    let dir = std::env::temp_dir().join(format!("delta-migrate-meta-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legacy.sqlite");
    let path_str = path.to_str().unwrap();

    let (session_id, main) = {
        // Build the database, insert a message carrying metadata, then physically
        // drop the four metadata columns so the file is a faithful pre-metadata
        // snapshot. Dropping after the insert proves the row predates the columns.
        let legacy = SqliteStore::open(path_str).unwrap();
        let (session, main) = legacy.register_session(new_session()).await.unwrap();
        legacy
            .upsert_messages(&[Message {
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
                model: Some("will-be-stripped".into()),
                git_branch: Some("will-be-stripped".into()),
                cwd: Some("will-be-stripped".into()),
                response_time_ms: Some(9400.0),
            }])
            .await
            .unwrap();
        let conn = legacy.conn.lock().await;
        // The FTS update trigger fires on a message UPDATE; DROP COLUMN does not
        // rewrite rows, so the columns can be removed without touching it.
        conn.execute_batch(
            "ALTER TABLE message DROP COLUMN model; \
             ALTER TABLE message DROP COLUMN git_branch; \
             ALTER TABLE message DROP COLUMN cwd; \
             ALTER TABLE message DROP COLUMN response_time_ms;",
        )
        .unwrap();
        (session.id, main)
    };

    // Re-opening applies the guarded ALTERs; the old row loads with NULL metadata
    // (the read path does not crash on the now-present-again columns) and keeps
    // its other fields intact.
    let store = SqliteStore::open(path_str).unwrap();
    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].uuid, MessageUuid::from("a-1"));
    assert_eq!(view[0].content_text.as_deref(), Some("answer"));
    assert_eq!(view[0].model, None, "a pre-migration row has no model");
    assert_eq!(view[0].git_branch, None);
    assert_eq!(view[0].cwd, None);
    assert_eq!(view[0].response_time_ms, None);

    // A fresh upsert of the same uuid now fills the metadata, proving the
    // migrated columns are writable.
    store
        .upsert_messages(&[Message {
            uuid: MessageUuid::from("a-1"),
            session_id: session_id.clone(),
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
            git_branch: None,
            cwd: None,
            response_time_ms: Some(1234.0),
        }])
        .await
        .unwrap();
    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view[0].model.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(view[0].response_time_ms, Some(1234.0));

    std::fs::remove_dir_all(&dir).ok();
}

/// The session-list page query is index-backed: its plan must walk
/// `ix_session_recency` and must NOT fall back to a full sort (temp b-tree).
/// Guards against a regression that reintroduces the O(total sessions) scan
/// (e.g. a correlated recency subquery or an ORDER BY the index can't satisfy).
#[tokio::test]
async fn list_sessions_page_uses_the_recency_index() {
    let store = SqliteStore::open_in_memory().unwrap();
    let conn = store.conn.lock().await;
    let plan: Vec<String> = conn
        .prepare(
            "EXPLAIN QUERY PLAN \
             SELECT id, cwd, transcript_path, title, status, created_at, \
                    last_activity_at, COALESCE(last_activity_at, created_at) AS recency \
             FROM session \
             WHERE NOT (status = 'spawning' AND last_activity_at IS NULL \
                        AND NOT EXISTS (SELECT 1 FROM message m WHERE m.session_id = session.id)) \
               AND (1 = 1) \
             ORDER BY recency DESC, created_at DESC, id DESC \
             LIMIT 10",
        )
        .unwrap()
        .query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let plan_text = plan.join("\n");
    assert!(
        plan_text.contains("ix_session_recency"),
        "page query should walk ix_session_recency, plan was:\n{plan_text}"
    );
    assert!(
        !plan_text.contains("USE TEMP B-TREE FOR ORDER BY"),
        "page query should not sort the whole table, plan was:\n{plan_text}"
    );
}

#[tokio::test]
async fn recent_workdirs_returns_distinct_cwds_in_recency_order() {
    let store = SqliteStore::open_in_memory().unwrap();

    let session_in = |id: &str, cwd: &str| NewSession {
        id: id.into(),
        cwd: cwd.into(),
        transcript_path: format!("/tmp/{id}.jsonl"),
        branch_at_launch: None,
        repo_root: None,
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
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
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
    // A session with no messages still contributes its workdir, keyed by its
    // own `created_at`, so a freshly-used directory is listed before any
    // message lands. With no `requested_workdir` set (the `register_session`
    // path never sets it), the query falls back to `cwd` for the workdir key.
    let (_s, _main) = store
        .register_session(NewSession {
            id: "sess-1".into(),
            cwd: "/fresh".into(),
            transcript_path: "/tmp/s.jsonl".into(),
            branch_at_launch: None,
            repo_root: None,
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
async fn recent_workdirs_returns_requested_workdir_not_worktree_cwd() {
    let store = SqliteStore::open_in_memory().unwrap();

    // Mirror a worktree-on spawn: `cwd` is the auto-generated worktree path
    // under `$DELTA_WORKTREE_BASE`, `requested_workdir` is the dir the user
    // picked (which is also the worktree's repo root). The Recent dirs query
    // must surface the user-selected dir, not the worktree path.
    let id = SessionId::from("sess-worktree");
    store
        .insert_spawning_session(
            &id,
            "/var/delta/worktrees/delta-sess-worktree",
            Some("delta-sess-worktree"),
            Some("/user-chosen"),
            Some("/user-chosen"),
        )
        .await
        .unwrap();

    let recent = store.recent_workdirs(10).await.unwrap();
    let paths: Vec<&str> = recent.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        vec!["/user-chosen"],
        "Recent surfaces the user-selected dir, not the auto-generated worktree path"
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
            model: None,
            git_branch: None,
            cwd: None,
            response_time_ms: None,
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
            model: None,
            git_branch: None,
            cwd: None,
            response_time_ms: None,
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
async fn list_sessions_page_excludes_message_less_spawning_sessions() {
    let store = SqliteStore::open_in_memory().unwrap();
    session_active_at(&store, "sess-live", "2026-01-01T00:00:00Z").await;
    let spawning = SessionId::from("sess-spawn");
    store
        .insert_spawning_session(&spawning, "/work", None, None, None)
        .await
        .unwrap();

    // The message-less spawning session stays out of the list: the browser
    // cannot open it, and the optimistic new-session chip must not bind to it.
    let page = store.list_sessions_page(None, 10).await.unwrap();
    assert_eq!(page_ids(&page), vec!["sess-live"]);

    // Activation (the first hook) makes it listable, exactly when a session
    // used to first appear.
    store
        .register_session(NewSession {
            id: spawning.clone(),
            cwd: "/work".into(),
            transcript_path: "/tmp/spawn.jsonl".into(),
            branch_at_launch: None,
            repo_root: None,
        })
        .await
        .unwrap();
    let page = store.list_sessions_page(None, 10).await.unwrap();
    assert_eq!(page.len(), 2, "the activated session is listed");
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
    // The PreToolUse row carries the correlating tool_use_id...
    let req = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, Some("toolu_01"))
        .await
        .unwrap();
    assert_eq!(req.tool_name, "Bash");
    assert_eq!(req.tool_use_id.as_deref(), Some("toolu_01"));
    assert!(req.id > 0);
    // ...and the PermissionRequest-owned dialog row records none (NULL, never
    // an empty-string sentinel).
    let dialog = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, None)
        .await
        .unwrap();
    assert_eq!(dialog.tool_use_id, None);
}

#[tokio::test]
async fn permission_request_resolves_by_tool_use_id() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    let req = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, Some("toolu_01"))
        .await
        .unwrap();

    // A non-matching tool_use_id resolves nothing.
    assert_eq!(
        store
            .resolve_permission_by_tool_use_id(&session.id, "toolu_other", true)
            .await
            .unwrap(),
        Vec::<i64>::new(),
    );

    // The matching, still-pending request resolves and returns its id.
    assert_eq!(
        store
            .resolve_permission_by_tool_use_id(&session.id, "toolu_01", true)
            .await
            .unwrap(),
        vec![req.id],
    );

    // A second resolve is a no-op: the request is no longer pending.
    assert_eq!(
        store
            .resolve_permission_by_tool_use_id(&session.id, "toolu_01", true)
            .await
            .unwrap(),
        Vec::<i64>::new(),
    );
}

#[tokio::test]
async fn resolve_settles_the_pending_dialog_row_too() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    // The PreToolUse row and the hook-owned dialog row for the same call.
    let pre = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"rm x"}"#, Some("toolu_01"))
        .await
        .unwrap();
    let dialog = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"rm x"}"#, None)
        .await
        .unwrap();

    // The tool_result settles both: the matching PreToolUse row and the
    // session's pending dialog row (the dialog blocked the session, so this
    // result is the one it gated).
    let mut resolved = store
        .resolve_permission_by_tool_use_id(&session.id, "toolu_01", false)
        .await
        .unwrap();
    resolved.sort_unstable();
    assert_eq!(resolved, vec![pre.id, dialog.id]);
}

#[tokio::test]
async fn decide_permission_request_decides_only_a_pending_row() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    let req = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, None)
        .await
        .unwrap();

    // The first decision lands: status + decided_at recorded, row returned.
    let decided = store
        .decide_permission_request(req.id, true)
        .await
        .unwrap()
        .expect("the pending row is decided");
    assert_eq!(decided.status, PermissionStatus::Allowed);
    assert!(decided.decided_at.is_some());
    assert_eq!(decided.session_id, session.id);

    // A second decision (or one for an unknown id) decides nothing.
    assert!(store
        .decide_permission_request(req.id, false)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .decide_permission_request(9999, true)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn queued_send_is_held_then_promoted_to_dispatched() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // A queued send is recorded but stays out of the outstanding (dispatched)
    // slot until it is promoted.
    let queued = store
        .enqueue_queued_send(&session.id, main, None, "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(queued.status, SendStatus::Queued);
    assert!(
        store.head_dispatched_send(&session.id).await.unwrap().is_none(),
        "a queued send is not a dispatched FIFO head"
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
        .head_dispatched_send(&session.id)
        .await
        .unwrap()
        .expect("the promoted send is now the outstanding dispatched send");
    assert_eq!(matched.id, queued.id);
    assert_eq!(matched.status, SendStatus::Dispatched);
    assert_eq!(matched.locator_quote.as_deref(), Some("quote"));
}

#[tokio::test]
async fn cancel_queued_send_only_cancels_while_queued() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();

    // A queued send cancels: the guarded transition reports it moved, the row is
    // terminal (`cancelled`), and it drops out of both the queue and the
    // open-send list so the idle dispatch path never reaches it.
    let queued = store
        .enqueue_queued_send(&session.id, main, None, "held", None)
        .await
        .unwrap();
    assert!(
        store.cancel_queued_send(queued.id).await.unwrap(),
        "a queued send transitions to cancelled"
    );
    assert_eq!(
        store.send(queued.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
    );
    assert!(
        store.next_queued_send(&session.id).await.unwrap().is_none(),
        "a cancelled send is skipped by the idle dispatch path"
    );
    assert!(
        store.open_sends(&session.id).await.unwrap().is_empty(),
        "a cancelled send drops out of the open-send list"
    );
    // A second cancel is now a no-op: the row already left `queued`.
    assert!(
        !store.cancel_queued_send(queued.id).await.unwrap(),
        "re-cancelling an already-cancelled send reports no transition"
    );

    // A dispatched send is not cancellable through the guarded path: the row
    // stays dispatched and the transition reports no change.
    let dispatched = store
        .enqueue_send(&session.id, main, None, "typed", None)
        .await
        .unwrap();
    assert_eq!(dispatched.status, SendStatus::Dispatched);
    assert!(
        !store.cancel_queued_send(dispatched.id).await.unwrap(),
        "a dispatched send is not cancellable while dispatched"
    );
    assert_eq!(
        store.send(dispatched.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the dispatched row is left untouched"
    );

    // An unknown id reports no transition rather than erroring.
    assert!(!store.cancel_queued_send(9999).await.unwrap());
}

#[tokio::test]
async fn open_sends_lists_non_terminal_sends_oldest_first_per_session() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    let (other, other_main) = store
        .register_session(new_session_with("sess-2"))
        .await
        .unwrap();

    // Mix of statuses for the session under test: a dispatched send, a queued
    // send, a matched one, and a cancelled one. Only the first two are open.
    let dispatched = store
        .enqueue_send(&session.id, main, None, "dispatched", None)
        .await
        .unwrap();
    let queued = store
        .enqueue_queued_send(&session.id, main, None, "queued", None)
        .await
        .unwrap();
    let matched = store
        .enqueue_send(&session.id, main, None, "matched", None)
        .await
        .unwrap();
    store
        .mark_send_matched(matched.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();
    let cancelled = store
        .enqueue_send(&session.id, main, None, "cancelled", None)
        .await
        .unwrap();
    store.cancel_send(cancelled.id).await.unwrap();
    // A foreign session's open send must never leak into this session's list.
    store
        .enqueue_send(&other.id, other_main, None, "foreign", None)
        .await
        .unwrap();

    let open = store.open_sends(&session.id).await.unwrap();
    let ids: Vec<i64> = open.iter().map(|s| s.id).collect();
    assert_eq!(
        ids,
        vec![dispatched.id, queued.id],
        "only queued/dispatched sends, oldest first"
    );
    assert_eq!(open[0].status, SendStatus::Dispatched);
    assert_eq!(open[1].status, SendStatus::Queued);

    // A session with no open sends yields an empty list, not an error.
    store
        .mark_send_matched(dispatched.id, &MessageUuid::from("u-2"))
        .await
        .unwrap();
    store.cancel_send(queued.id).await.unwrap();
    assert!(store.open_sends(&session.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn spawning_session_inserts_then_activates_on_register() {
    let store = SqliteStore::open_in_memory().unwrap();
    let id = SessionId::from("sess-spawn");

    // The eager insert: status `spawning`, no transcript path yet, and the
    // main thread already created so a first send can target real ids.
    let (session, main) = store
        .insert_spawning_session(&id, "/work", None, None, None)
        .await
        .unwrap();
    assert_eq!(session.status, SessionStatus::Spawning);
    assert_eq!(session.transcript_path, None);
    assert_eq!(store.main_thread_id(&id).await.unwrap(), main);

    // The first hook activates the row: status flips and the hook-reported
    // transcript path is filled in; the main thread is reused, not duplicated.
    let (activated, main2) = store
        .register_session(NewSession {
            id: id.clone(),
            cwd: "/work/real".into(),
            transcript_path: "/tmp/spawn.jsonl".into(),
            branch_at_launch: None,
            repo_root: None,
        })
        .await
        .unwrap();
    assert_eq!(activated.status, SessionStatus::Active);
    assert_eq!(activated.transcript_path.as_deref(), Some("/tmp/spawn.jsonl"));
    assert_eq!(activated.cwd, "/work/real");
    assert_eq!(main2, main, "the eagerly-created main thread is reused");

    // A later re-registration must not clobber the activated row.
    let (again, _) = store
        .register_session(NewSession {
            id: id.clone(),
            cwd: "/elsewhere".into(),
            transcript_path: "/tmp/other.jsonl".into(),
            branch_at_launch: None,
            repo_root: None,
        })
        .await
        .unwrap();
    assert_eq!(again.transcript_path.as_deref(), Some("/tmp/spawn.jsonl"));
    assert_eq!(again.cwd, "/work/real");
}

#[tokio::test]
async fn delete_session_cascades_to_children() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    store
        .enqueue_send(&session.id, main, None, "hello", None)
        .await
        .unwrap();
    store
        .upsert_messages(&[Message {
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
        }])
        .await
        .unwrap();
    store.set_transcript_lines_read(&session.id, 3).await.unwrap();

    store.delete_session(&session.id).await.unwrap();

    // The row and everything it owned are gone.
    assert!(store.session(&session.id).await.unwrap().is_none());
    assert!(store.list_threads(&session.id).await.unwrap().is_empty());
    assert_eq!(store.message_count(&session.id).await.unwrap(), 0);
    assert!(store
        .head_dispatched_send(&session.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(store.transcript_lines_read(&session.id).await.unwrap(), 0);
}

#[tokio::test]
async fn mark_session_failed_flips_only_a_spawning_session() {
    let store = SqliteStore::open_in_memory().unwrap();

    // A spawning session fails.
    let id = SessionId::from("sess-spawn");
    store
        .insert_spawning_session(&id, "/work", None, None, None)
        .await
        .unwrap();
    store.mark_session_failed(&id).await.unwrap();
    let failed = store.session(&id).await.unwrap().unwrap();
    assert_eq!(failed.status, SessionStatus::Failed);

    // An active session is untouched by a stale failure mark.
    let (active, _) = store.register_session(new_session()).await.unwrap();
    store.mark_session_failed(&active.id).await.unwrap();
    let still = store.session(&active.id).await.unwrap().unwrap();
    assert_eq!(still.status, SessionStatus::Active);
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

#[tokio::test]
async fn launch_options_round_trip_create_list_delete() {
    let store = SqliteStore::open_in_memory().unwrap();

    // A fresh store has no registered launch options.
    assert!(store.list_launch_options().await.unwrap().is_empty());

    // A flag with a label and a value persists every field, including the
    // pre-checked `default_enabled` flag.
    let plugin = store
        .create_launch_option(Some("My plugins"), "--plugin-dir", Some("/opt/plugins"), true)
        .await
        .unwrap();
    assert_eq!(plugin.label.as_deref(), Some("My plugins"));
    assert_eq!(plugin.name, "--plugin-dir");
    assert_eq!(plugin.value.as_deref(), Some("/opt/plugins"));
    assert!(plugin.default_enabled);
    assert!(!plugin.created_at.is_empty());

    // A valueless, unlabeled flag stores NULL for both — never a sentinel — and
    // `default_enabled` defaults to off.
    let valueless = store
        .create_launch_option(None, "--dangerously-skip-permissions", None, false)
        .await
        .unwrap();
    assert_eq!(valueless.label, None);
    assert_eq!(valueless.value, None);
    assert!(!valueless.default_enabled);
    assert_ne!(valueless.id, plugin.id, "ids are distinct");

    // The persisted `default_enabled` round-trips through `list`.
    let listed_plugin = store
        .list_launch_options()
        .await
        .unwrap()
        .into_iter()
        .find(|o| o.id == plugin.id)
        .unwrap();
    assert!(listed_plugin.default_enabled);

    // The list is newest-first (descending id), so the second insert leads.
    let listed = store.list_launch_options().await.unwrap();
    let ids: Vec<i64> = listed.iter().map(|o| o.id).collect();
    assert_eq!(ids, vec![valueless.id, plugin.id]);

    // Deleting one leaves the other untouched.
    store.delete_launch_option(plugin.id).await.unwrap();
    let remaining = store.list_launch_options().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, valueless.id);

    // Deleting an unknown id is a silent no-op (idempotent), not an error.
    store.delete_launch_option(9999).await.unwrap();
    assert_eq!(store.list_launch_options().await.unwrap().len(), 1);
}

#[tokio::test]
async fn set_launch_option_default_enabled_toggles_in_place() {
    let store = SqliteStore::open_in_memory().unwrap();
    let option = store
        .create_launch_option(None, "--plugin-dir", Some("/opt/plugins"), false)
        .await
        .unwrap();
    assert!(!option.default_enabled);

    // Toggling on returns the updated row, preserving id and created_at.
    let updated = store
        .set_launch_option_default_enabled(option.id, true)
        .await
        .unwrap()
        .expect("an existing option");
    assert_eq!(updated.id, option.id);
    assert_eq!(updated.created_at, option.created_at);
    assert!(updated.default_enabled);

    // The change persists.
    let listed = store.list_launch_options().await.unwrap();
    assert!(listed[0].default_enabled);

    // Toggling back off works too.
    let updated = store
        .set_launch_option_default_enabled(option.id, false)
        .await
        .unwrap()
        .expect("an existing option");
    assert!(!updated.default_enabled);

    // Toggling an unknown id returns None rather than erroring.
    assert!(store
        .set_launch_option_default_enabled(9999, true)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn subagent_launches_round_trip_and_clear() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    let child = store
        .create_thread(&session.id, "side topic", Some(main))
        .await
        .unwrap()
        .id;

    // No launches recorded yet.
    assert!(store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap()
        .is_empty());

    // Record two launches against different threads.
    store
        .record_subagent_launch(&session.id, "toolu_a", child)
        .await
        .unwrap();
    store
        .record_subagent_launch(&session.id, "toolu_b", main)
        .await
        .unwrap();
    let launches = store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap();
    assert_eq!(
        launches.get("toolu_a").map(|launch| launch.thread_id),
        Some(child)
    );
    assert_eq!(
        launches.get("toolu_b").map(|launch| launch.thread_id),
        Some(main)
    );
    assert!(
        launches.values().all(|launch| launch.task_id.is_none()),
        "a fresh launch carries no task_id until upgrade_subagent_task_id runs"
    );

    // Upgrading an entry sets its task_id; re-record keeps that upgrade.
    store
        .upgrade_subagent_task_id(&session.id, "toolu_a", "a31425032172620ed")
        .await
        .unwrap();
    assert_eq!(
        store
            .outstanding_subagent_launches(&session.id)
            .await
            .unwrap()
            .get("toolu_a")
            .and_then(|launch| launch.task_id.clone()),
        Some("a31425032172620ed".to_owned())
    );

    // Re-recording the same id refreshes the thread rather than erroring, and
    // must NOT wipe the previously-upgraded task_id.
    store
        .record_subagent_launch(&session.id, "toolu_a", main)
        .await
        .unwrap();
    let after = store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap();
    assert_eq!(after.get("toolu_a").map(|launch| launch.thread_id), Some(main));
    assert_eq!(
        after.get("toolu_a").and_then(|launch| launch.task_id.clone()),
        Some("a31425032172620ed".to_owned()),
        "the previously-upgraded task_id survives a re-record"
    );

    // Upgrading an unknown id is a silent no-op (the launch may have already
    // been folded by its completion notification).
    store
        .upgrade_subagent_task_id(&session.id, "toolu_unknown", "anything")
        .await
        .unwrap();

    // Clearing one leaves the other; clearing an unknown id is a no-op.
    store
        .clear_subagent_launch(&session.id, "toolu_a")
        .await
        .unwrap();
    store
        .clear_subagent_launch(&session.id, "nonexistent")
        .await
        .unwrap();
    let remaining = store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining.get("toolu_b").map(|launch| launch.thread_id),
        Some(main)
    );
}

/// An existing database created before `subagent_launch.task_id` existed must
/// gain the column on open, with pre-existing rows surfacing `task_id: None`.
/// This is the additive-column smoke test that pins the recovery path the
/// task-id-fallback fix relies on for already-deployed databases.
#[tokio::test]
async fn opening_a_pre_subagent_task_id_database_migrates_and_loads_old_rows_as_null() {
    let dir = std::env::temp_dir().join(format!("delta-subagent-migrate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legacy.sqlite");
    let path_str = path.to_str().unwrap();

    // Build a "legacy" database: open through the store (which now adds
    // `task_id`), seed a launch row, then physically drop the column so the
    // file looks pre-migration.
    let main = {
        let legacy = SqliteStore::open(path_str).unwrap();
        legacy.register_session(new_session()).await.unwrap();
        let main = legacy.main_thread_id(&SessionId::from("sess-1")).await.unwrap();
        legacy
            .record_subagent_launch(&SessionId::from("sess-1"), "toolu_legacy", main)
            .await
            .unwrap();
        // Strip the column so the file is a faithful pre-migration snapshot.
        let conn = legacy.conn.lock().await;
        conn.execute_batch("ALTER TABLE subagent_launch DROP COLUMN task_id;")
            .unwrap();
        main
    };

    // Re-opening applies the guarded ALTER. The legacy row keeps its
    // thread_id and surfaces a NULL task_id, so the fold still seeds the
    // launch correctly — it just lacks the fallback correlation key, which is
    // the historical behaviour anyway.
    let store = SqliteStore::open(path_str).unwrap();
    let launches = store
        .outstanding_subagent_launches(&SessionId::from("sess-1"))
        .await
        .unwrap();
    let legacy = launches.get("toolu_legacy").expect("legacy launch survives migration");
    assert_eq!(legacy.thread_id, main);
    assert!(
        legacy.task_id.is_none(),
        "a pre-migration row migrates as NULL task_id"
    );

    // A subsequent upgrade fills the column for new launches.
    store
        .upgrade_subagent_task_id(&SessionId::from("sess-1"), "toolu_legacy", "agent_abc")
        .await
        .unwrap();
    let after = store
        .outstanding_subagent_launches(&SessionId::from("sess-1"))
        .await
        .unwrap();
    assert_eq!(
        after.get("toolu_legacy").and_then(|l| l.task_id.clone()),
        Some("agent_abc".to_owned())
    );

    std::fs::remove_dir_all(&dir).ok();
}

// SCHEMA_VERSION startup gate. The four cases below are the contract from the
// compatibility policy doc (subdomain 1): a fresh DB stamps current; a pre-gate
// v0.1.0 DB is silently rescued; any other non-matching version is refused with
// an error naming `make reset`; a current DB opens normally.

/// Read the on-disk `PRAGMA user_version` from a freshly-opened connection,
/// so the gate's effect on a file can be observed independently of the store
/// (which holds its own connection behind an async mutex).
fn read_user_version(path: &str) -> u32 {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

#[tokio::test]
async fn schema_gate_stamps_a_fresh_database_with_the_current_version() {
    let dir = std::env::temp_dir().join(format!("delta-schema-fresh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fresh.sqlite");
    let path_str = path.to_str().unwrap();

    // Sanity: the file does not exist yet, so the open is genuinely creating
    // it. `user_version` defaults to 0 on a brand-new SQLite file.
    assert!(!path.exists());

    let store = SqliteStore::open(path_str).unwrap();
    // The store works as usual (the schema steps ran).
    store.register_session(new_session()).await.unwrap();
    drop(store);

    // The gate stamped the file current, so a re-open takes the match path.
    assert_eq!(read_user_version(path_str), crate::SCHEMA_VERSION);
    let reopened = SqliteStore::open(path_str).unwrap();
    let again = reopened
        .session(&SessionId::from("sess-1"))
        .await
        .unwrap();
    assert!(again.is_some(), "the stamped DB re-opens normally");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn schema_gate_rescues_a_pre_gate_v0_1_0_database() {
    let dir =
        std::env::temp_dir().join(format!("delta-schema-rescue-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legacy.sqlite");
    let path_str = path.to_str().unwrap();

    // Build a "shipped-as-v0.1.0" DB: open it through the store, write some
    // overlay state, then reset `user_version` to 0 so the file looks exactly
    // like one created before the gate landed. The `session` table is present,
    // which is the marker the rescue branch keys off.
    {
        let store = SqliteStore::open(path_str).unwrap();
        store.register_session(new_session()).await.unwrap();
        let conn = store.conn.lock().await;
        conn.execute_batch("PRAGMA user_version = 0").unwrap();
    }
    assert_eq!(read_user_version(path_str), 0);

    // Re-opening must NOT error — the rescue branch silently bumps the marker
    // and continues. The pre-existing overlay row is still there afterwards.
    let store = SqliteStore::open(path_str).unwrap();
    assert!(store
        .session(&SessionId::from("sess-1"))
        .await
        .unwrap()
        .is_some());
    drop(store);

    // The file is now stamped current, so a second re-open is the match path.
    assert_eq!(read_user_version(path_str), crate::SCHEMA_VERSION);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn schema_gate_refuses_a_non_matching_version() {
    let dir =
        std::env::temp_dir().join(format!("delta-schema-mismatch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("future.sqlite");
    let path_str = path.to_str().unwrap();

    // Build a DB then force `user_version` to a value that is neither 0 nor
    // the current SCHEMA_VERSION. This stands in for both "future binary
    // wrote it" and "stale overlay against newer binary" — the gate makes no
    // distinction; it refuses both.
    {
        let store = SqliteStore::open(path_str).unwrap();
        store.register_session(new_session()).await.unwrap();
        let conn = store.conn.lock().await;
        let foreign = crate::SCHEMA_VERSION + 1;
        conn.execute_batch(&format!("PRAGMA user_version = {foreign}"))
            .unwrap();
    }

    let err = match SqliteStore::open(path_str) {
        Ok(_) => panic!("expected mismatched version to be refused"),
        Err(err) => err,
    };
    match &err {
        crate::Error::SchemaMismatch { found, expected } => {
            assert_eq!(*expected, crate::SCHEMA_VERSION);
            assert_eq!(*found, crate::SCHEMA_VERSION + 1);
        }
        other => panic!("expected SchemaMismatch, got {other:?}"),
    }
    // The error's `Display` is what reaches the user — it must name
    // `make reset` so the remediation is obvious without consulting docs.
    let rendered = err.to_string();
    assert!(
        rendered.contains("make reset"),
        "error message must name `make reset`: {rendered}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn schema_gate_opens_a_current_database_unchanged() {
    let dir =
        std::env::temp_dir().join(format!("delta-schema-match-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("current.sqlite");
    let path_str = path.to_str().unwrap();

    // First open stamps the file current; a second open is the match branch.
    {
        let store = SqliteStore::open(path_str).unwrap();
        store.register_session(new_session()).await.unwrap();
    }
    assert_eq!(read_user_version(path_str), crate::SCHEMA_VERSION);

    let store = SqliteStore::open(path_str).unwrap();
    let session = store
        .session(&SessionId::from("sess-1"))
        .await
        .unwrap()
        .expect("the registered session survives a re-open");
    assert_eq!(session.id.as_str(), "sess-1");

    // The version did not change (idempotent on the match path).
    drop(store);
    assert_eq!(read_user_version(path_str), crate::SCHEMA_VERSION);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn repository_clone_rows_aggregates_by_repo_root_and_requested_workdir() {
    let store = SqliteStore::open_in_memory().unwrap();

    // Two sessions at the same (repo_root, requested_workdir) — the second is
    // more recent and on a different branch. A third session at the SAME repo
    // root but a different requested_workdir is its own clone row. A fourth
    // session is outside any git repo (no repo_root) and must be excluded.
    let s1 = SessionId::from("sess-1");
    store
        .insert_spawning_session(
            &s1,
            "/repo-a/wt-1",
            Some("main"),
            Some("/repo-a"),
            Some("/repo-a"),
        )
        .await
        .unwrap();
    let s2 = SessionId::from("sess-2");
    store
        .insert_spawning_session(
            &s2,
            "/repo-a/wt-2",
            Some("feature/x"),
            Some("/repo-a"),
            Some("/repo-a"),
        )
        .await
        .unwrap();
    let s3 = SessionId::from("sess-3");
    store
        .insert_spawning_session(
            &s3,
            "/repo-a-mirror",
            Some("main"),
            Some("/repo-a"),
            Some("/repo-a-mirror"),
        )
        .await
        .unwrap();
    let s4 = SessionId::from("sess-4");
    store
        .insert_spawning_session(&s4, "/scratch", None, None, Some("/scratch"))
        .await
        .unwrap();

    // Stamp `last_activity_at` for s1 and s2 explicitly so s2 is the latest at
    // its `(repo_root, requested_workdir)` pair, driving the `last_branch`
    // pick. The default `created_at` is `now`, which is later than any
    // hard-coded past timestamp, so without explicit stamps s1 would sort
    // newer than s2 by `COALESCE(last_activity_at, created_at)`.
    let mk_msg = |session_id: SessionId, thread_id: ThreadId, uuid: &str, at: &str| Message {
        uuid: MessageUuid::from(uuid),
        session_id,
        thread_id,
        role: Role::User,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq: 0,
        content_text: Some("hi".into()),
        content: vec![ContentBlock::Text { text: "hi".into() }],
        created_at: Some(at.into()),
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
    };
    let s1_thread = store.main_thread_id(&s1).await.unwrap();
    let s2_thread = store.main_thread_id(&s2).await.unwrap();
    store
        .upsert_messages(&[
            mk_msg(s1.clone(), s1_thread, "m-s1", "2026-01-01T00:00:00Z"),
            mk_msg(s2.clone(), s2_thread, "m-s2", "2026-02-01T00:00:00Z"),
        ])
        .await
        .unwrap();

    let rows = store.repository_clone_rows().await.unwrap();
    assert_eq!(rows.len(), 2, "non-git session is excluded; one row per pair");

    // Find each row by its clone path.
    let a = rows
        .iter()
        .find(|r| r.clone_path == "/repo-a")
        .expect("the bundled /repo-a clone is present");
    assert_eq!(a.repo_root, "/repo-a");
    assert_eq!(
        a.last_branch.as_deref(),
        Some("feature/x"),
        "the latest session at this pair (s2) contributes last_branch"
    );
    assert_eq!(
        a.last_opened_at.as_deref(),
        Some("2026-02-01T00:00:00Z"),
        "last_opened_at uses the max recency across the pair's sessions"
    );

    let mirror = rows
        .iter()
        .find(|r| r.clone_path == "/repo-a-mirror")
        .expect("the second clone of /repo-a is its own row");
    assert_eq!(mirror.repo_root, "/repo-a");
    assert_eq!(mirror.last_branch.as_deref(), Some("main"));
}
