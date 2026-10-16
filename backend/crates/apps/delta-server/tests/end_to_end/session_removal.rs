//! `DELETE /api/sessions/{id}`: removing a closed session, and refusing an
//! open or still-starting one.

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::support::{build_app, delete, get, listed_session_ids, post_json};

/// `DELETE /api/sessions/{id}` removes a **closed** session: it answers `204`
/// and the session, its thread and its message are gone from every surface that
/// served them a moment earlier.
///
/// The session ran outside any Delta worktree, so what is removed is Delta's
/// own data and nothing else (the worktree rules are pinned in the use-case
/// suite), and the agent's own transcript is never touched — this test's temp
/// transcript file is still on disk when it cleans it up below.
///
/// The removal is also announced on the event stream, which is the only way a
/// tab that did not issue it learns the row is gone.
#[tokio::test]
async fn removing_a_closed_session_deletes_it_and_its_rows() {
    let (app, _tmux, transcript_path, state) = build_app();
    let transcript_str = transcript_path.to_str().unwrap().to_owned();
    let session_id = "sess-remove";

    // Register the session through its first hook, then close its registration
    // turn: a hook-registered session has a row and no live pane, which is the
    // one state `Remove` is offered in.
    let (status, _) = post_json(
        &app,
        "/hooks/user-prompt-submit",
        json!({
            "prompt": "hello there",
            "session_id": session_id,
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post_json(&app, "/hooks/stop", json!({ "session_id": session_id })).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sessions"][0]["open"], false, "closed, so removable");
    let (status, threads) = get(&app, &format!("/api/sessions/{session_id}/threads")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        threads["threads"].as_array().expect("threads array").len(),
        1,
        "the `main` thread exists before the removal"
    );

    // Subscribed before the call, since a broadcast subscriber only sees what
    // is sent after it subscribes.
    let mut events = state.subscribe();
    let (status, body) = delete(&app, &format!("/api/sessions/{session_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null, "a 204 carries no body");

    // Every other tab is still rendering the card, and the `204` reaches only
    // the tab that asked: this event is what tells the rest to drop it.
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("the removal is announced within the timeout")
            .expect("the broadcast channel is open"),
        delta_usecase::SessionEvent::SessionRemoved {
            session_id: delta_usecase::SessionId::from(session_id),
        },
        "announced as its own event, never as a close"
    );

    assert!(
        listed_session_ids(&app).await.is_empty(),
        "the removed session is gone from the list"
    );
    // Its children went with it: the session is unknown to every route that
    // addresses it by id.
    let (status, _) = get(&app, &format!("/api/sessions/{session_id}/threads")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Removing it again is a clean `404`, not a silent success: the id is now
    // unknown, exactly as a never-seen one is.
    let (status, _) = delete(&app, &format!("/api/sessions/{session_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = delete(&app, "/api/sessions/never-existed").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    assert!(
        transcript_path.exists(),
        "the agent's own transcript is not Delta's to delete"
    );
    let _ = std::fs::remove_file(&transcript_path);
}

/// An **open** session is refused with `409` and the stable `session_open`
/// code — idle or mid-turn — and the row is untouched either way.
///
/// The browser offers `Remove` only on a closed card, so this answers a stale
/// one (another tab reopened the session) rather than anything a user meets in
/// normal use.
#[tokio::test]
async fn removing_an_open_session_is_refused_with_its_stable_code() {
    let (app, _tmux, transcript_path, state) = build_app();
    let transcript_str = transcript_path.to_str().unwrap().to_owned();
    let session_id = "sess-open";

    let (status, _) = post_json(
        &app,
        "/hooks/user-prompt-submit",
        json!({
            "prompt": "hello there",
            "session_id": session_id,
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post_json(&app, "/hooks/stop", json!({ "session_id": session_id })).await;
    assert_eq!(status, StatusCode::OK);

    // A send to the closed session resumes it, which binds a live pane: the
    // session is now open and idle.
    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let main_thread_id = body["sessions"][0]["main_thread_id"]
        .as_i64()
        .expect("main thread id");
    let (status, _) = post_json(
        &app,
        "/api/sends",
        json!({ "thread_id": main_thread_id, "text": "are you there?" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sessions"][0]["open"], true, "the resume bound a pane");

    let mut events = state.subscribe();
    let (status, refused) = delete(&app, &format!("/api/sessions/{session_id}")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        refused["code"], "session_open",
        "distinct from `session_spawning`: the answer is `close it first`"
    );
    assert_eq!(
        listed_session_ids(&app).await,
        vec![session_id.to_owned()],
        "a refused removal deletes nothing"
    );
    // The handler broadcasts only after the use case has answered, so a refusal
    // announces nothing: no watching tab drops a card that is still there.
    assert!(
        events.try_recv().is_err(),
        "a refused removal announces no event"
    );

    // Mid-turn is the same refusal, and the turn is not interrupted by it.
    let (status, _) = post_json(
        &app,
        "/hooks/user-prompt-submit",
        json!({
            "prompt": "are you there?",
            "session_id": session_id,
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, refused) = delete(&app, &format!("/api/sessions/{session_id}")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "session_open");
    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["sessions"][0]["session"]["id"], session_id,
        "the row survives the mid-turn refusal too"
    );
    assert_eq!(
        body["sessions"][0]["open"], true,
        "the refusal leaves the session open; it does not close it as a side effect"
    );

    let _ = std::fs::remove_file(&transcript_path);
}

/// A session that is **still starting** is refused with `409` and the existing
/// `session_spawning` code, and its row is left for the launch to bind (or for
/// a close to cancel).
#[tokio::test]
async fn removing_a_still_starting_session_is_refused_with_its_stable_code() {
    let (app, _tmux, transcript_path, _state) = build_app();

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "text": "kick off a new conversation",
            "workdir": "/tmp/delta-e2e-workdir",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("a real session id")
        .to_owned();

    // The launch has run to a pane awaiting its first hook: the row says
    // `spawning` and nothing is bound to it.
    let (status, list) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["sessions"][0]["session"]["status"], "spawning");
    assert_eq!(list["sessions"][0]["open"], false);

    let (status, refused) = delete(&app, &format!("/api/sessions/{session_id}")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        refused["code"], "session_spawning",
        "the answer here is `wait for it to come up`, not `close it first`"
    );
    assert_eq!(
        listed_session_ids(&app).await,
        vec![session_id],
        "a refused removal deletes nothing"
    );

    let _ = std::fs::remove_file(&transcript_path);
}
