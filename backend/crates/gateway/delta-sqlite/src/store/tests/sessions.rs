//! Session registration, lifecycle, list paging, and workdir/repository queries.

use delta_model::{
    AgentProvider, ContentBlock, Message, MessageUuid, Role, SessionId, SessionStatus, ThreadId,
};
use delta_usecase::{NewSession, SessionPageCursor};

use super::super::SqliteStore;
use super::{new_session, new_session_with};

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

/// A freshly inserted session round-trips its provider, and the
/// provider-minted conversation ids written later via `set_provider_ids`.
#[tokio::test]
async fn a_spawning_session_round_trips_provider_fields() {
    let store = SqliteStore::open_in_memory().unwrap();

    // A Claude spawn (every existing caller) reads back as Claude with no
    // provider ids — the behaviour before this change.
    store
        .insert_spawning_session(
            &SessionId::from("claude-1"),
            "/work",
            None,
            None,
            None,
            None,
            AgentProvider::Claude,
            None,
        )
        .await
        .unwrap();
    let claude = store
        .session(&SessionId::from("claude-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claude.provider, AgentProvider::Claude);
    assert_eq!(claude.provider_session_id, None);
    assert_eq!(claude.provider_thread_id, None);

    // A Codex spawn records its provider; the provider-minted ids are unknown
    // at spawn (NULL) and are filled in later via `set_provider_ids`.
    let codex_id = SessionId::from("codex-1");
    store
        .insert_spawning_session(
            &codex_id,
            "/work",
            None,
            None,
            None,
            None,
            AgentProvider::Codex,
            None,
        )
        .await
        .unwrap();
    let spawned = store.session(&codex_id).await.unwrap().unwrap();
    assert_eq!(spawned.provider, AgentProvider::Codex);
    assert_eq!(spawned.provider_session_id, None);
    assert_eq!(spawned.provider_thread_id, None);

    store
        .set_provider_ids(&codex_id, Some("thr_abc"), Some("thr_abc"))
        .await
        .unwrap();
    let resolved = store.session(&codex_id).await.unwrap().unwrap();
    assert_eq!(resolved.provider, AgentProvider::Codex);
    assert_eq!(resolved.provider_session_id.as_deref(), Some("thr_abc"));
    assert_eq!(resolved.provider_thread_id.as_deref(), Some("thr_abc"));
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
             WHERE (1 = 1) \
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
        repository_display_name: None,
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
        provider_item_id: None,
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
            repository_display_name: None,
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

/// The PR a session was opened from is persisted by the spawning insert and
/// read back by both session read paths — the single-session `get` and the
/// recency-ordered list the navigator renders. A spawn with no PR origin stores
/// NULL, which is what the card renders as an empty slot.
#[tokio::test]
async fn a_spawning_session_round_trips_its_pull_request_number() {
    let store = SqliteStore::open_in_memory().unwrap();

    let from_pr = SessionId::from("sess-pr");
    let (inserted, _main) = store
        .insert_spawning_session(
            &from_pr,
            "/work",
            Some("feat/repo-tab"),
            Some("/work"),
            Some("/work"),
            Some("x7c1/delta"),
            AgentProvider::Claude,
            Some(138),
        )
        .await
        .unwrap();
    assert_eq!(
        inserted.pull_request_number,
        Some(138),
        "the returned row carries the number without a re-read"
    );

    let from_directory = SessionId::from("sess-dir");
    store
        .insert_spawning_session(
            &from_directory,
            "/work",
            None,
            None,
            None,
            None,
            AgentProvider::Claude,
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        store
            .session(&from_pr)
            .await
            .unwrap()
            .unwrap()
            .pull_request_number,
        Some(138),
    );
    assert_eq!(
        store
            .session(&from_directory)
            .await
            .unwrap()
            .unwrap()
            .pull_request_number,
        None,
        "a session started from a directory records no PR origin"
    );

    // The list page is the navigator's own read path, so it must carry the
    // number too — the card is rendered from a list row, never from a `get`.
    let page = store.list_sessions_page(None, 10).await.unwrap();
    let numbers: Vec<(&str, Option<i64>)> = page
        .iter()
        .map(|(session, _)| (session.id.as_str(), session.pull_request_number))
        .collect();
    assert!(numbers.contains(&("sess-pr", Some(138))), "{numbers:?}");
    assert!(numbers.contains(&("sess-dir", None)), "{numbers:?}");
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
            None,
            AgentProvider::Claude,
            None,
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
            provider_item_id: None,
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
    let second = store.list_sessions_page(Some(cursor), 2).await.unwrap();
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
async fn list_sessions_page_lists_message_less_spawning_sessions() {
    let store = SqliteStore::open_in_memory().unwrap();
    session_active_at(&store, "sess-live", "2026-01-01T00:00:00Z").await;
    let spawning = SessionId::from("sess-spawn");
    store
        .insert_spawning_session(
            &spawning,
            "/work",
            None,
            None,
            None,
            None,
            AgentProvider::Claude,
            None,
        )
        .await
        .unwrap();

    // A session is listed from the moment its first send is accepted — the
    // eager `spawning` row, before any hook — so the browser can focus it
    // right away. It has no activity, so it keys on its own (just-now)
    // `created_at` and sorts above the older active session.
    let page = store.list_sessions_page(None, 10).await.unwrap();
    assert_eq!(page_ids(&page), vec!["sess-spawn", "sess-live"]);
    let spawning_row = page.iter().find(|(s, _)| s.id == spawning).unwrap();
    assert_eq!(spawning_row.0.status, SessionStatus::Spawning);

    // Activation (the first hook) only changes the status; the row stays where
    // it already was.
    store
        .register_session(NewSession {
            id: spawning.clone(),
            cwd: "/work".into(),
            transcript_path: "/tmp/spawn.jsonl".into(),
            branch_at_launch: None,
            repo_root: None,
            repository_display_name: None,
        })
        .await
        .unwrap();
    let page = store.list_sessions_page(None, 10).await.unwrap();
    assert_eq!(page_ids(&page), vec!["sess-spawn", "sess-live"]);
    let activated = page.iter().find(|(s, _)| s.id == spawning).unwrap();
    assert_eq!(activated.0.status, SessionStatus::Active);
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
async fn spawning_session_inserts_then_activates_on_register() {
    let store = SqliteStore::open_in_memory().unwrap();
    let id = SessionId::from("sess-spawn");

    // The eager insert: status `spawning`, no transcript path yet, and the
    // main thread already created so a first send can target real ids.
    let (session, main) = store
        .insert_spawning_session(
            &id,
            "/work",
            None,
            None,
            None,
            None,
            AgentProvider::Claude,
            None,
        )
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
            repository_display_name: None,
        })
        .await
        .unwrap();
    assert_eq!(activated.status, SessionStatus::Active);
    assert_eq!(
        activated.transcript_path.as_deref(),
        Some("/tmp/spawn.jsonl")
    );
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
            repository_display_name: None,
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
            provider_item_id: None,
        }])
        .await
        .unwrap();
    store
        .set_transcript_lines_read(&session.id, 3)
        .await
        .unwrap();

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
        .insert_spawning_session(
            &id,
            "/work",
            None,
            None,
            None,
            None,
            AgentProvider::Claude,
            None,
        )
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
            None,
            AgentProvider::Claude,
            None,
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
            None,
            AgentProvider::Claude,
            None,
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
            None,
            AgentProvider::Claude,
            None,
        )
        .await
        .unwrap();
    let s4 = SessionId::from("sess-4");
    store
        .insert_spawning_session(
            &s4,
            "/scratch",
            None,
            None,
            Some("/scratch"),
            None,
            AgentProvider::Claude,
            None,
        )
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
        provider_item_id: None,
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

    let rows = store
        .repository_clone_rows("/no/such/worktree-base", 20, 5, 10)
        .await
        .unwrap();
    assert_eq!(
        rows.len(),
        2,
        "non-git session is excluded; one row per pair"
    );

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
