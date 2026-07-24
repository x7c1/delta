//! Schema migrations and the `SCHEMA_VERSION` startup gate.

use delta_model::{AgentProvider, ContentBlock, Message, MessageUuid, Role, SendStatus, SessionId};
use delta_usecase::NewSession;

use super::super::SqliteStore;
use super::new_session;

/// A database created before `send.restored_at` existed must gain the column
/// on open and load its pre-existing rows with the field as NULL — never
/// crashing on the now-wider `send_from_row` and never losing the row's other
/// data. NULL is exactly the "not restored" meaning, so pre-upgrade queued
/// rows keep dispatching normally.
#[tokio::test]
async fn opening_a_pre_restored_at_database_migrates_and_loads_old_rows_as_null() {
    let dir = std::env::temp_dir().join(format!("delta-migrate-restored-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legacy.sqlite");
    let path_str = path.to_str().unwrap();

    let (session_id, queued_id) = {
        // Build the database, record a queued send, then physically drop the
        // column so the file is a faithful pre-`restored_at` snapshot.
        let legacy = SqliteStore::open(path_str).unwrap();
        let (session, main) = legacy.register_session(new_session()).await.unwrap();
        let queued = legacy
            .enqueue_queued_send(&session.id, main, None, "held", None)
            .await
            .unwrap();
        let conn = legacy.conn.lock().await;
        conn.execute_batch("ALTER TABLE send DROP COLUMN restored_at")
            .unwrap();
        (session.id, queued.id)
    };

    // Re-opening applies the guarded ALTER; the old row loads as unrestored
    // and stays on the normal queued path.
    let store = SqliteStore::open(path_str).unwrap();
    let queued = store.send(queued_id).await.unwrap().unwrap();
    assert_eq!(queued.status, SendStatus::Queued);
    assert_eq!(
        queued.restored_at, None,
        "a pre-migration row is unrestored"
    );
    assert_eq!(
        store
            .next_queued_send(&session_id)
            .await
            .unwrap()
            .expect("a pre-migration queued row still dispatches normally")
            .id,
        queued_id,
    );

    std::fs::remove_dir_all(&dir).ok();
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
                repository_display_name: None,
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
                repository_display_name: None,
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
            provider_item_id: None,
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
                provider_item_id: None,
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
            provider_item_id: None,
        }])
        .await
        .unwrap();
    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view[0].model.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(view[0].response_time_ms, Some(1234.0));

    std::fs::remove_dir_all(&dir).ok();
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
        let main = legacy
            .main_thread_id(&SessionId::from("sess-1"))
            .await
            .unwrap();
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
    let legacy = launches
        .get("toolu_legacy")
        .expect("legacy launch survives migration");
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

/// An existing database created before the multi-provider columns
/// (`session.provider`/`provider_session_id`/`provider_thread_id` and
/// `launch_option.provider`) existed must gain them on open, with every
/// pre-existing row reading back as a Claude row: `provider = 'claude'` and the
/// nullable provider-minted ids NULL. These columns are additive and unused by
/// the C1 runtime — this test pins that an already-deployed database opens
/// cleanly and its rows keep their historical (Claude) meaning after migration.
///
/// The "legacy" file is built by hand with the original pre-additive table
/// shapes (`session`/`launch_option` carrying only their base columns) and
/// `user_version = 0`, then opened through the store: the guarded
/// `ADDITIVE_COLUMNS` steps add every additive column — the provider ones under
/// test plus the earlier ones (`last_activity_at`, `default_enabled`, …) — so
/// this exercises the real recovery path a genuinely old database takes.
/// (`DROP COLUMN` cannot faithfully undo the columns here: SQLite's schema
/// re-parse chokes on the `--plugin-dir` token already present in the
/// `launch_option` comment.)
#[tokio::test]
async fn opening_a_pre_provider_database_migrates_and_loads_old_rows_as_claude() {
    let dir = std::env::temp_dir().join(format!("delta-provider-migrate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legacy.sqlite");
    let path_str = path.to_str().unwrap();

    // Build a faithful pre-additive database directly: only the base columns,
    // `user_version = 0` (untouched), one session and one launch-option row.
    {
        let conn = rusqlite::Connection::open(path_str).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (
               id              TEXT PRIMARY KEY,
               cwd             TEXT NOT NULL,
               transcript_path TEXT,
               title           TEXT,
               status          TEXT NOT NULL
                                 CHECK (status IN ('spawning','active','ended','failed')),
               created_at      TEXT NOT NULL
             ) STRICT;
             CREATE TABLE launch_option (
               id         INTEGER PRIMARY KEY,
               label      TEXT,
               name       TEXT NOT NULL,
               value      TEXT,
               created_at TEXT NOT NULL
             ) STRICT;
             INSERT INTO session (id, cwd, transcript_path, title, status, created_at)
               VALUES ('sess-1', '/work', '/tmp/t.jsonl', NULL, 'active', '2026-01-01T00:00:00Z');
             INSERT INTO launch_option (label, name, value, created_at)
               VALUES ('plugins', '--plugin-dir', '/plug', '2026-01-01T00:00:00Z');",
        )
        .unwrap();
    }

    // Opening through the store applies the guarded ALTERs; it must open cleanly.
    let store = SqliteStore::open(path_str).unwrap();

    // The domain read now surfaces the provider fields (added to `Session` in
    // C3a): a pre-migration row loads as a Claude session with no
    // provider-minted ids, and `map_session` does not choke on the now-wider
    // column set.
    let session = store
        .session(&SessionId::from("sess-1"))
        .await
        .unwrap()
        .expect("pre-provider session still loads");
    assert_eq!(session.provider, AgentProvider::Claude);
    assert_eq!(session.provider_session_id, None);
    assert_eq!(session.provider_thread_id, None);

    // Also confirm the migrated values straight from SQLite, including
    // `launch_option.provider`, which has no domain read path yet.
    let conn = store.conn.lock().await;
    let (provider, provider_session_id, provider_thread_id): (
        String,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT provider, provider_session_id, provider_thread_id \
             FROM session WHERE id = 'sess-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        provider, "claude",
        "a pre-migration session is a Claude row"
    );
    assert_eq!(
        provider_session_id, None,
        "a pre-migration Claude session has no provider-minted session id"
    );
    assert_eq!(
        provider_thread_id, None,
        "a pre-migration Claude session has no provider-minted thread id"
    );

    let option_provider: String = conn
        .query_row(
            "SELECT provider FROM launch_option WHERE name = '--plugin-dir'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        option_provider, "claude",
        "a pre-migration launch option is a Claude option"
    );
    drop(conn);

    // Re-opening the now-migrated database is a clean no-op (the columns already
    // exist, so the guarded ALTERs do not run again).
    SqliteStore::open(path_str).unwrap();

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
    let again = reopened.session(&SessionId::from("sess-1")).await.unwrap();
    assert!(again.is_some(), "the stamped DB re-opens normally");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn schema_gate_rescues_a_pre_gate_v0_1_0_database() {
    let dir = std::env::temp_dir().join(format!("delta-schema-rescue-{}", std::process::id()));
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
    let dir = std::env::temp_dir().join(format!("delta-schema-mismatch-{}", std::process::id()));
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
    let dir = std::env::temp_dir().join(format!("delta-schema-match-{}", std::process::id()));
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
