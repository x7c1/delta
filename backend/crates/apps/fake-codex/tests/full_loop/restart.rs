//! Resume across a server restart: a second backend over the same on-disk
//! database reconnects the session and continues its persisted conversation.

use axum::http::StatusCode;
use serde_json::json;

use delta_sqlite::SqliteStore;

use crate::support::{build_app_with, drain_one_turn, get, post_json, ScenarioGuard};

/// A unique temp path for a shared on-disk SQLite database, removed on drop. The
/// restart test opens it from two separate backends (before/after the simulated
/// restart), so it must outlive both — unlike the in-memory store every other
/// test uses.
struct DbGuard {
    path: std::path::PathBuf,
}

impl DbGuard {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fake-codex-restart-{}-{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::remove_file(&path).ok();
        Self { path }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path.to_string_lossy()).unwrap()
    }
}

impl Drop for DbGuard {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
        // WAL sidecar files.
        std::fs::remove_file(self.path.with_extension("db-wal")).ok();
        std::fs::remove_file(self.path.with_extension("db-shm")).ok();
    }
}

/// A one-turn scenario that streams `reply` from a distinct `turn_id`/`item_id`,
/// so two successive turns (the second after a restart) produce distinct,
/// non-colliding provider items. The provider thread id is fixed to `thr_restart`
/// across both, so the resume reattaches to the same thread.
///
/// `model` is what this backend's app-server reports for the thread. The two
/// halves of the restart test give distinct values, so the post-restart messages
/// prove the metadata came from the **resume** response rather than from
/// anything cached before the restart.
fn restart_turn_scenario(turn_id: &str, item_id: &str, reply: &str, model: &str) -> ScenarioGuard {
    ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_restart",
            "model": "{model}",
            "turn": {{
                "turn_id": "{turn_id}",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "{item_id}", "type": "agentMessage" }} }},
                    {{ "type": "item_completed", "item": {{ "id": "{item_id}", "type": "agentMessage", "text": "{reply}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    ))
}

/// The Codex **resume-across-restart** full loop: create a session and complete a
/// turn, then boot a SECOND backend over the SAME on-disk database with a fresh
/// interactor (no in-process bindings — the post-restart state) and a distinct
/// scenario, and send another message to the same thread.
///
/// This is the regression proof for dogfooding gap #1: after a server restart the
/// live `codex app-server` connection + thread + bound `open_agent` are gone, so
/// a send to a previously-created Codex session used to take the Claude resume
/// path (`ensure_open` → `claude --resume`) and fail with `ResumeUnavailable`
/// (surfaced as `409`). The fix reconnects the session over the adapter via
/// `thread/resume` (reattaching to the SAME provider thread) and re-seeds the
/// content source at the persisted message count, so the second turn dispatches,
/// streams, and completes, and the persisted conversation **continues**: the
/// first turn's assistant reply is preserved and the second's is appended with
/// the next sequence number — no renumber, no duplicate.
#[tokio::test(flavor = "multi_thread")]
async fn codex_resume_across_restart_continues_the_persisted_conversation() {
    const REPLY_ONE: &str = "reply from turn one";
    const REPLY_TWO: &str = "reply from turn two";
    // Distinct per backend, so a post-restart message reporting `MODEL_TWO` can
    // only have learned it from the `thread/resume` response.
    const MODEL_ONE: &str = "model-before-restart";
    const MODEL_TWO: &str = "model-after-restart";
    let db = DbGuard::new();

    // ---- Before the restart: create the session, complete turn 1. ----
    let scenario1 = restart_turn_scenario("turn_one", "item_one", REPLY_ONE, MODEL_ONE);
    let (thread_id, session_id) = {
        let (app, state) = build_app_with(db.open(), &scenario1);
        let mut events = state.subscribe();
        state
            .spawn_async_event_drain()
            .expect("the async drain is taken exactly once");

        let (status, body) = post_json(
            &app,
            "/api/sends",
            json!({ "new_session": true, "provider": "codex", "text": "first message" }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "the first send was created: {body:?}"
        );
        let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();
        let thread_id = body["send"]["thread_id"].as_i64().unwrap();

        // Let turn 1 stream and complete, so its assistant reply is persisted.
        drain_one_turn(&mut events, &session_id).await;
        let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
        assert_eq!(status, StatusCode::OK, "turn 1 messages fetched: {body:?}");
        assert!(
            body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["content_text"] == json!(REPLY_ONE)),
            "turn 1's assistant reply persisted before the restart: {body:?}"
        );
        (thread_id, session_id)
        // `app`/`state` (and the turn-1 `fake-codex` subprocess) drop here — the
        // server going away.
    };

    // ---- After the restart: a brand-new backend over the SAME database. ----
    let scenario2 = restart_turn_scenario("turn_two", "item_two", REPLY_TWO, MODEL_TWO);
    let (app, state) = build_app_with(db.open(), &scenario2);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // The second send targets the SAME thread. Before the fix this returned `409`
    // (ResumeUnavailable via the Claude path); after it must reconnect over the
    // adapter and be accepted.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "thread_id": thread_id, "text": "second message" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the post-restart send resumed the Codex session over the adapter (no 409): {body:?}"
    );
    assert_eq!(
        body["send"]["session_id"].as_str().unwrap(),
        session_id,
        "the resumed send stays on the same session"
    );

    // Turn 2 streams and completes over the reconnected pump.
    drain_one_turn(&mut events, &session_id).await;

    // The persisted conversation continued: turn 1's reply is still there and
    // turn 2's is appended, on contiguous sequence numbers with no duplicate —
    // proof the content source was re-seeded at the persisted count, not 0.
    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "post-restart messages fetched: {body:?}"
    );
    let messages = body["messages"].as_array().unwrap();

    // All four messages of the two-turn conversation are present: turn 1's user
    // prompt + assistant reply (preserved across the restart) and turn 2's user
    // prompt + assistant reply (appended after the resume). None was overwritten.
    for expected in ["first message", REPLY_ONE, "second message", REPLY_TWO] {
        assert!(
            messages
                .iter()
                .any(|m| m["content_text"] == json!(expected)),
            "`{expected}` is present in the continued conversation: {messages:?}"
        );
    }
    assert_eq!(
        messages.len(),
        4,
        "the conversation has exactly its four messages — nothing lost or duplicated: {messages:?}"
    );

    // Sequence numbers are contiguous with no duplicate: history was extended,
    // not renumbered.
    let mut seqs: Vec<i64> = messages
        .iter()
        .map(|m| m["seq"].as_i64().unwrap())
        .collect();
    seqs.sort_unstable();
    assert_eq!(
        seqs,
        vec![0, 1, 2, 3],
        "sequence numbers continue contiguously across the resume: {messages:?}"
    );

    // A resumed session still reports its provider metadata: `thread/resume`
    // carries the same required top-level `model` as `thread/start`, so the
    // post-restart turn's messages are stamped from the resume response — the
    // second backend's model, never left blank and never the pre-restart one.
    // The pre-restart messages keep the model that was running when they were
    // folded, since each row records what produced it.
    let model_of = |text: &str| {
        messages
            .iter()
            .find(|m| m["content_text"] == json!(text))
            .unwrap_or_else(|| panic!("`{text}` is present"))["model"]
            .clone()
    };
    assert_eq!(
        model_of(REPLY_TWO),
        json!(MODEL_TWO),
        "the resumed turn reports the model its `thread/resume` announced: {messages:?}"
    );
    assert_eq!(
        model_of(REPLY_ONE),
        json!(MODEL_ONE),
        "the pre-restart turn keeps the model that produced it: {messages:?}"
    );
}
