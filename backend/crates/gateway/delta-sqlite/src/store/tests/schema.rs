//! The startup schema gate, and what a later migration step does to rows that
//! were already there.
//!
//! The ladder's own mechanics — ordering, transactions, backups — are tested in
//! `crate::migrations::tests` against synthetic ladders. What is pinned here is
//! the store's behaviour around them: which databases it opens, which it
//! refuses, and that a row written at the current baseline still reads back
//! correctly (with any newly added column NULL) once a later step has been
//! applied over it. That last part is the real shape of every past additive
//! column change — `send.restored_at`, the `message` metadata columns,
//! `subagent_launch.task_id`, the provider columns — now that they are all part
//! of the v3 baseline and can no longer be simulated by dropping them.

use delta_model::{AgentProvider, ContentBlock, Message, MessageUuid, Role, SendStatus, SessionId};

use super::super::SqliteStore;
use super::new_session;
use crate::migrations::{migrate, Step};
use crate::SCHEMA_VERSION;

/// Apply a synthetic next-version step to an open store, standing in for the
/// first real migration that will ride a later change.
///
/// The step is additive and touches only the named table, so nothing else in the
/// store's behaviour can explain a row that fails to read back afterwards.
async fn apply_next_step(store: &SqliteStore, sql: &'static str) {
    let conn = store.conn.lock().await;
    let steps = [Step::additive(SCHEMA_VERSION + 1, sql)];
    migrate(
        &conn,
        &steps,
        SCHEMA_VERSION,
        SCHEMA_VERSION + 1,
        // In-memory: there is no file to snapshot, and an additive step would
        // not ask for one anyway.
        None,
    )
    .expect("the synthetic step applies");
}

/// Read a nullable TEXT column with no bound parameters, straight from SQLite —
/// the added columns below have no domain read path.
async fn read_optional_text(store: &SqliteStore, sql: &str) -> Option<String> {
    let conn = store.conn.lock().await;
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

/// A queued send recorded before a later step must survive it, keep every field
/// it had, and stay on the normal dispatch path — a newly added column reading
/// NULL is exactly the "this row predates the column" meaning.
#[tokio::test]
async fn a_send_recorded_before_a_later_step_survives_and_still_dispatches() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    let queued = store
        .enqueue_queued_send(&session.id, main, None, "held", None)
        .await
        .unwrap();

    apply_next_step(&store, "ALTER TABLE send ADD COLUMN dispatch_note TEXT;").await;

    let reloaded = store.send(queued.id).await.unwrap().unwrap();
    assert_eq!(reloaded.status, SendStatus::Queued);
    assert_eq!(reloaded.text, "held");
    assert_eq!(
        reloaded.restored_at, None,
        "a queued send that was never restored stays unrestored"
    );
    assert_eq!(
        store
            .next_queued_send(&session.id)
            .await
            .unwrap()
            .expect("the pre-migration queued row still dispatches normally")
            .id,
        queued.id,
    );
    assert_eq!(
        read_optional_text(&store, "SELECT dispatch_note FROM send").await,
        None,
        "the column added by the later step reads NULL on the older row",
    );
}

/// A message ingested before a later step must keep its body and its metadata,
/// and load through the normal read path with the newly added column NULL.
#[tokio::test]
async fn a_message_ingested_before_a_later_step_keeps_its_metadata() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    store
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
            model: Some("claude-opus-4-8".into()),
            git_branch: Some("main".into()),
            cwd: Some("/work".into()),
            response_time_ms: Some(9400.0),
            provider_item_id: None,
        }])
        .await
        .unwrap();

    apply_next_step(&store, "ALTER TABLE message ADD COLUMN token_cost REAL;").await;

    let view = store.thread_messages(main).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].uuid, MessageUuid::from("a-1"));
    assert_eq!(view[0].content_text.as_deref(), Some("answer"));
    assert_eq!(view[0].model.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(view[0].git_branch.as_deref(), Some("main"));
    assert_eq!(view[0].cwd.as_deref(), Some("/work"));
    assert_eq!(view[0].response_time_ms, Some(9400.0));
    assert_eq!(view[0].provider_item_id, None);
    assert_eq!(
        read_optional_text(&store, "SELECT CAST(token_cost AS TEXT) FROM message").await,
        None,
        "the column added by the later step reads NULL on the older row",
    );

    // The row is still writable through the normal upsert path afterwards.
    store
        .upsert_messages(&[Message {
            uuid: MessageUuid::from("a-1"),
            session_id: session.id.clone(),
            thread_id: main,
            role: Role::Assistant,
            linear_parent_uuid: None,
            semantic_parent_uuid: None,
            prompt_id: None,
            seq: 0,
            content_text: Some("edited".into()),
            content: vec![ContentBlock::Text {
                text: "edited".into(),
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
    assert_eq!(view[0].content_text.as_deref(), Some("edited"));
    assert_eq!(view[0].response_time_ms, Some(1234.0));
}

/// An outstanding subagent launch recorded before a later step keeps its
/// thread attribution — the whole point of the row — and can still be upgraded
/// with the task id the `PostToolUse(Agent)` hook learns later.
#[tokio::test]
async fn a_subagent_launch_recorded_before_a_later_step_keeps_its_thread() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    store
        .record_subagent_launch(&session.id, "toolu_legacy", main)
        .await
        .unwrap();

    apply_next_step(
        &store,
        "ALTER TABLE subagent_launch ADD COLUMN launched_by TEXT;",
    )
    .await;

    let launches = store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap();
    let launch = launches
        .get("toolu_legacy")
        .expect("the launch survives the migration");
    assert_eq!(launch.thread_id, main);
    assert!(
        launch.task_id.is_none(),
        "a launch whose PostToolUse has not landed yet has no task id"
    );

    store
        .upgrade_subagent_task_id(&session.id, "toolu_legacy", "agent_abc")
        .await
        .unwrap();
    let after = store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap();
    assert_eq!(
        after.get("toolu_legacy").and_then(|l| l.task_id.clone()),
        Some("agent_abc".to_owned())
    );
    assert_eq!(
        read_optional_text(&store, "SELECT launched_by FROM subagent_launch").await,
        None,
        "the column added by the later step reads NULL on the older row",
    );
}

/// A session and a launch option written before a later step still read back as
/// Claude rows: `provider = 'claude'` and no provider-minted ids. These columns
/// carry a constant `NOT NULL DEFAULT`, so the historical meaning survives any
/// later step without a backfill.
#[tokio::test]
async fn a_session_written_before_a_later_step_stays_a_claude_row() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _main) = store.register_session(new_session()).await.unwrap();
    store
        .create_launch_option(
            Some("plugins"),
            "--plugin-dir",
            Some("/plug"),
            false,
            AgentProvider::Claude,
        )
        .await
        .unwrap();

    apply_next_step(&store, "ALTER TABLE session ADD COLUMN closed_at TEXT;").await;

    let reloaded = store
        .session(&session.id)
        .await
        .unwrap()
        .expect("the session still loads");
    assert_eq!(reloaded.provider, AgentProvider::Claude);
    assert_eq!(reloaded.provider_session_id, None);
    assert_eq!(reloaded.provider_thread_id, None);
    assert_eq!(
        read_optional_text(&store, "SELECT closed_at FROM session").await,
        None,
        "the column added by the later step reads NULL on the older row",
    );

    let options = store
        .list_launch_options()
        .await
        .expect("the launch option registry still loads");
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].provider, AgentProvider::Claude);
}

/// A database written by the previous generation — stamped 3, with no
/// `prompt_template` table — is migrated forward on open: the new table is
/// created, the file is re-stamped, and the launch options that were already
/// registered are still there.
#[tokio::test]
async fn a_v3_database_gains_the_prompt_template_table_and_keeps_its_launch_options() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3.sqlite");
    let path_str = path.to_str().unwrap();

    // Build a v3 file: open at the current version, register a launch option,
    // then undo the v4 step (drop the table, restamp) so the file is exactly
    // what the previous generation left behind.
    {
        let store = SqliteStore::open(path_str).unwrap();
        store
            .create_launch_option(
                Some("plugins"),
                "--plugin-dir",
                Some("/opt/plugins"),
                true,
                AgentProvider::Claude,
            )
            .await
            .unwrap();
        let conn = store.conn.lock().await;
        conn.execute_batch("DROP TABLE prompt_template; PRAGMA user_version = 3;")
            .unwrap();
    }
    assert_eq!(read_user_version(path_str), 3);

    let store = SqliteStore::open(path_str).unwrap();

    // The pending step ran and the file is current again.
    assert_eq!(read_user_version(path_str), crate::SCHEMA_VERSION);

    // The registry the migration was *not* about is untouched.
    let options = store.list_launch_options().await.unwrap();
    assert_eq!(options.len(), 1, "the pre-existing launch option survives");
    assert_eq!(options[0].name, "--plugin-dir");
    assert_eq!(options[0].value.as_deref(), Some("/opt/plugins"));
    assert!(options[0].default_enabled);

    // The new table exists and is usable — empty, since the ladder creates it
    // rather than backfilling anything.
    assert!(store.list_prompt_templates().await.unwrap().is_empty());
    let created = store
        .create_prompt_template("Merge", "Once CI is green, merge.")
        .await
        .unwrap();
    assert_eq!(
        store.list_prompt_templates().await.unwrap(),
        vec![created],
        "the migrated database writes and reads templates normally"
    );
}

// The startup gate. The cases below are the contract from the compatibility
// policy doc (subdomain 1): a fresh DB is built by replaying the ladder and
// stamped current; a current DB opens with nothing applied; a pre-gate v0.1.0
// DB, a DB stamped below the ladder's baseline, and a DB from a newer binary
// are all refused with an error naming `make reset`.

/// Read the on-disk `PRAGMA user_version` from a freshly-opened connection,
/// so the gate's effect on a file can be observed independently of the store
/// (which holds its own connection behind an async mutex).
fn read_user_version(path: &str) -> u32 {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

/// Every `delta.db.bak-*` snapshot sitting next to the database.
fn backup_files(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".bak-v"))
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn schema_gate_stamps_a_fresh_database_with_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.sqlite");
    let path_str = path.to_str().unwrap();

    // Sanity: the file does not exist yet, so the open is genuinely creating
    // it. `user_version` defaults to 0 on a brand-new SQLite file.
    assert!(!path.exists());

    let store = SqliteStore::open(path_str).unwrap();
    // The store works as usual — the whole ladder replayed, so the schema is
    // there.
    store.register_session(new_session()).await.unwrap();
    drop(store);

    // The last step stamped the file current, so a re-open takes the match path.
    assert_eq!(read_user_version(path_str), crate::SCHEMA_VERSION);
    let reopened = SqliteStore::open(path_str).unwrap();
    let again = reopened.session(&SessionId::from("sess-1")).await.unwrap();
    assert!(again.is_some(), "the stamped DB re-opens normally");
}

/// A database already at the current version must be opened with nothing
/// applied: no step runs, no snapshot is written, and the stamp is left alone.
/// This is what makes the migration machinery safe to land inert.
#[tokio::test]
async fn schema_gate_opens_a_current_database_without_applying_anything() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("current.sqlite");
    let path_str = path.to_str().unwrap();

    // First open replays the ladder; a second open is the match branch.
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
    drop(store);

    assert_eq!(
        read_user_version(path_str),
        crate::SCHEMA_VERSION,
        "the version is not rewritten on the match path"
    );
    assert!(
        backup_files(dir.path()).is_empty(),
        "an up-to-date database is never snapshotted: {:?}",
        backup_files(dir.path()),
    );
}

/// A `user_version = 0` database that already has delta's tables predates the
/// gate entirely. Its real shape is unknown, so the baseline cannot be replayed
/// onto it and there is no version to migrate forward from: refuse to open,
/// naming `make reset`.
#[tokio::test]
async fn schema_gate_refuses_a_pre_gate_v0_1_0_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let path_str = path.to_str().unwrap();

    // Build a "shipped-as-v0.1.0" DB: open it through the store, write some
    // overlay state, then reset `user_version` to 0 so the file looks exactly
    // like one created before the gate landed. The `session` table is present,
    // which is the marker the refusal keys off.
    {
        let store = SqliteStore::open(path_str).unwrap();
        store.register_session(new_session()).await.unwrap();
        let conn = store.conn.lock().await;
        conn.execute_batch("PRAGMA user_version = 0").unwrap();
    }
    assert_eq!(read_user_version(path_str), 0);

    let err = match SqliteStore::open(path_str) {
        Ok(_) => panic!("expected an unstamped overlay to be refused"),
        Err(err) => err,
    };
    assert!(
        matches!(err, crate::Error::UnstampedOverlay),
        "expected UnstampedOverlay, got {err:?}"
    );
    // The error's `Display` is what reaches the user — it must name
    // `make reset` so the remediation is obvious without consulting docs.
    let rendered = err.to_string();
    assert!(
        rendered.contains("make reset"),
        "error message must name `make reset`: {rendered}"
    );

    // Nothing was written on the way out: the refusal happens before any step.
    assert_eq!(read_user_version(path_str), 0);
    assert!(backup_files(dir.path()).is_empty());
}

/// A database stamped *below* the ladder's oldest step — v0.2.x and v0.3.0 both
/// shipped stamping 1 — has no step that carries it forward, because those
/// generations were squashed into the baseline rather than reconstructed.
/// Replaying the baseline over it would apply nothing (`IF NOT EXISTS` over
/// tables that already exist) and then stamp the older shape as current, so the
/// gate refuses it instead, naming `make reset`.
#[tokio::test]
async fn schema_gate_refuses_a_database_older_than_the_ladders_baseline() {
    let baseline = crate::migrations::registry()[0].to_version;
    assert!(
        baseline > 1,
        "this case only exists while the ladder starts above 1",
    );
    let stale = baseline - 1;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stale.sqlite");
    let path_str = path.to_str().unwrap();

    // Build a database with delta's tables and stamp it one generation below the
    // baseline, the way a v0.3.0 overlay reaches this binary.
    {
        let store = SqliteStore::open(path_str).unwrap();
        store.register_session(new_session()).await.unwrap();
        let conn = store.conn.lock().await;
        conn.execute_batch(&format!("PRAGMA user_version = {stale}"))
            .unwrap();
    }

    let err = match SqliteStore::open(path_str) {
        Ok(_) => panic!("expected a pre-baseline overlay to be refused"),
        Err(err) => err,
    };
    match &err {
        crate::Error::PreBaselineOverlay {
            found,
            baseline: reported,
        } => {
            assert_eq!(*found, stale);
            assert_eq!(*reported, baseline);
        }
        other => panic!("expected PreBaselineOverlay, got {other:?}"),
    }
    let rendered = err.to_string();
    assert!(
        rendered.contains("make reset"),
        "error message must name `make reset`: {rendered}"
    );

    // Nothing was written on the way out — above all, the file was not stamped
    // current while still carrying its older shape.
    assert_eq!(read_user_version(path_str), stale);
    assert!(backup_files(dir.path()).is_empty());
}

/// A database stamped *above* this binary's version was written by a newer
/// delta. The ladder only runs forward, so this stays a hard refusal.
#[tokio::test]
async fn schema_gate_refuses_a_database_from_a_newer_binary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.sqlite");
    let path_str = path.to_str().unwrap();

    {
        let store = SqliteStore::open(path_str).unwrap();
        store.register_session(new_session()).await.unwrap();
        let conn = store.conn.lock().await;
        let foreign = crate::SCHEMA_VERSION + 1;
        conn.execute_batch(&format!("PRAGMA user_version = {foreign}"))
            .unwrap();
    }

    let err = match SqliteStore::open(path_str) {
        Ok(_) => panic!("expected a newer version to be refused"),
        Err(err) => err,
    };
    match &err {
        crate::Error::SchemaMismatch { found, expected } => {
            assert_eq!(*expected, crate::SCHEMA_VERSION);
            assert_eq!(*found, crate::SCHEMA_VERSION + 1);
        }
        other => panic!("expected SchemaMismatch, got {other:?}"),
    }
    let rendered = err.to_string();
    assert!(
        rendered.contains("make reset"),
        "error message must name `make reset`: {rendered}"
    );
}
