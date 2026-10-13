//! The whole flow: a session registered by its first hook, a send into its
//! thread correlated against the turn, and the thread and message state that
//! results.

use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::support::{build_app, get, post_json, TICK_BOUND};

#[tokio::test]
async fn drives_session_send_and_turn_correlation_end_to_end() {
    let (app, tmux, transcript_path, state) = build_app();
    let transcript_str = transcript_path.to_str().unwrap().to_owned();
    let session_id = "sess-e2e";

    // 1. First UserPromptSubmit registers the session. The transcript is empty,
    //    so this prompt is treated as external input and returns no context.
    let (status, body) = post_json(
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
    assert!(
        body.get("additionalContext").is_none(),
        "no pending send queued yet, so no additionalContext"
    );

    // That first prompt started a turn; complete it with a Stop so the session
    // is idle before the next send. Otherwise the quoted send below would be
    // queued (held back behind the in-flight turn) rather than dispatched.
    let (status, _) = post_json(&app, "/hooks/stop", json!({ "session_id": session_id })).await;
    assert_eq!(status, StatusCode::OK);

    // 2. GET /api/sessions lists the registered session, annotated with its open
    //    state and main thread id. It registered via a hook (not a Delta spawn),
    //    so it is a known-but-closed data session.
    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions.len(), 1, "exactly the one registered session");
    assert_eq!(sessions[0]["session"]["id"], session_id);
    assert_eq!(sessions[0]["session"]["cwd"], "/work/delta");
    assert_eq!(
        sessions[0]["open"], false,
        "a hook-registered (external) session has no live pane"
    );
    assert!(
        sessions[0]["last_activity_at"].is_null(),
        "a freshly registered session has no messages yet"
    );
    assert!(
        body["next_cursor"].is_null(),
        "a single short page has no next cursor"
    );
    let main_thread_id = sessions[0]["main_thread_id"]
        .as_i64()
        .expect("main thread id");

    // 3. POST /api/sends enqueues a send into the main thread, with a locator
    //    quote that must surface as additionalContext on the matching turn.
    let prompt = "what is a delta?";
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "thread_id": main_thread_id,
            "text": prompt,
            "locator_quote": "the main channel",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["send"]["text"], prompt);
    assert_eq!(body["send"]["status"], "dispatched");
    assert_eq!(body["send"]["thread_id"].as_i64(), Some(main_thread_id));
    let send_id = body["send"]["id"].as_i64().expect("dispatched send id");
    // The session was hook-registered (closed), so this send resumed it via
    // `claude --resume`. A resumed session holds its first prompt until its
    // `SessionStart(source=resume)` readiness hook arrives, so the keystroke has
    // NOT been dispatched yet.
    assert_eq!(
        tmux.sent.load(Ordering::SeqCst),
        0,
        "the resume's first prompt is held until SessionStart(resume)"
    );

    // GET /api/sessions/{id}/sends mirrors the queue: the dispatched send is
    // the session's one open send, so the browser's pending strip can render
    // it from server state.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
    assert_eq!(status, StatusCode::OK);
    let sends = body["sends"].as_array().expect("sends array");
    assert_eq!(sends.len(), 1, "the dispatched send is open");
    assert_eq!(sends[0]["id"].as_i64(), Some(send_id));
    assert_eq!(sends[0]["text"], prompt);
    assert_eq!(sends[0]["status"], "dispatched");
    assert_eq!(sends[0]["thread_id"].as_i64(), Some(main_thread_id));

    // Feed the readiness hook. SessionStart(source=resume) blocks `claude` until
    // its handler returns, so the handler only *marks* the resume ready — it does
    // NOT type the held prompt (a keystroke sent from inside the hook would be
    // lost to a still-blocked TUI). So nothing is dispatched yet.
    let (status, _) = post_json(
        &app,
        "/hooks/session-start",
        json!({
            "session_id": session_id,
            "source": "resume",
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        tmux.sent.load(Ordering::SeqCst),
        0,
        "the readiness hook only marks ready; it does not dispatch from the handler"
    );

    // The held first prompt is dispatched on the background tick, a beat after the
    // hook returned and `claude` is input-ready. Drive that tick directly with a
    // `now` past the dispatch settle (the server loop normally calls this with
    // `Instant::now()`); the keystroke now lands on the normal `send_line` path.
    state
        .interactor()
        .dispatch_ready_resumes(Instant::now() + Duration::from_secs(1), TICK_BOUND)
        .await
        .unwrap();
    assert_eq!(
        tmux.sent.load(Ordering::SeqCst),
        1,
        "the held first prompt dispatched on the settle tick once the resume was ready"
    );

    // 4. The session emits the corresponding transcript line. Append it so the
    //    correlation can find the matched uuid.
    let matched_uuid = "uuid-of-the-question";
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript_path)
            .unwrap();
        let line = json!({
            "uuid": matched_uuid,
            "parentUuid": Value::Null,
            "type": "user",
            "promptId": "prompt-1",
            "timestamp": "2026-01-01T00:00:00Z",
            "message": { "role": "user", "content": prompt },
        });
        writeln!(file, "{line}").unwrap();
        file.flush().unwrap();
    }

    // The matching UserPromptSubmit fires: the FIFO head matches, the locator
    // quote is returned in the hookSpecificOutput envelope, and the line is
    // ingested.
    let (status, body) = post_json(
        &app,
        "/hooks/user-prompt-submit",
        json!({
            "prompt": prompt,
            "session_id": session_id,
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Claude Code only consumes injected context from the `hookSpecificOutput`
    // envelope, so the matched quote must surface nested there.
    assert_eq!(
        body["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit",
        "the envelope names the originating hook event"
    );
    // The quote is not injected verbatim: it is wrapped in a short frame so the
    // model recognises it as a passage the user selected from earlier in the
    // conversation, anchoring the current message. Because the send carries a
    // locator quote, the frame is followed by a note binding that passage to the
    // thread the conversation is now in (`thread:N`).
    assert_eq!(
        body["hookSpecificOutput"]["additionalContext"],
        format!(
            "The user is replying to this passage they selected from earlier in the conversation:\n\"the main channel\"\nThat passage starts a separate thread (thread:{main_thread_id}); the user is now talking in that thread."
        ),
        "matched send injects its locator quote, framed, as additionalContext"
    );

    // The matched send is terminal, so the session's open-send list is empty
    // again — the pending strip drains from server state.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["sends"].as_array().expect("sends array").is_empty(),
        "a matched send no longer lists as open"
    );

    // 5. GET /api/sessions/{id}/threads exposes the main thread for the
    //    navigator, scoped to this session.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/threads")).await;
    assert_eq!(status, StatusCode::OK);
    let threads = body["threads"].as_array().expect("threads array");
    assert_eq!(threads.len(), 1, "only the main thread exists");
    assert_eq!(threads[0]["id"].as_i64(), Some(main_thread_id));
    assert_eq!(threads[0]["title"], "main");

    // GET /api/threads/{id}/messages reflects the ingested, correlated message.
    let (status, body) = get(&app, &format!("/api/threads/{main_thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK);
    let messages = body["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 1, "the question was ingested once");
    let message = &messages[0];
    assert_eq!(message["uuid"], matched_uuid);
    assert_eq!(message["role"], "user");
    assert_eq!(message["thread_id"].as_i64(), Some(main_thread_id));
    assert_eq!(message["content_text"], prompt);

    // A miss against the FIFO (no remaining pending send) is external input.
    let (status, body) = post_json(
        &app,
        "/hooks/user-prompt-submit",
        json!({
            "prompt": "typed straight into the pane",
            "session_id": session_id,
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("additionalContext").is_none());

    // No further keystrokes were dispatched by the hook path.
    assert_eq!(tmux.sent.load(Ordering::SeqCst), 1);

    // The matched send id remained stable across the flow.
    assert!(send_id > 0);

    // 6. Close the session: the pane is torn down (here a no-op, since this
    //    external session never had a live pane) but the data is kept. It still
    //    lists, now as closed.
    let (status, _) = post_json(
        &app,
        &format!("/api/sessions/{session_id}/close"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions.len(), 1, "closing keeps the session in the store");
    assert_eq!(sessions[0]["open"], false, "the session lists as closed");
    assert_eq!(
        sessions[0]["last_activity_at"], "2026-01-01T00:00:00Z",
        "last_activity_at reflects the ingested message's timestamp"
    );

    // Open/close/threads on an unknown session id are 404.
    let (status, _) = post_json(&app, "/api/sessions/ghost/open", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "open of unknown id is 404");
    let (status, _) = post_json(&app, "/api/sessions/ghost/close", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "close of unknown id is 404");
    let (status, _) = get(&app, "/api/sessions/ghost/threads").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "threads of unknown id is 404"
    );
    let (status, _) = get(&app, "/api/sessions/ghost/sends").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "sends of unknown id is 404");

    let _ = std::fs::remove_file(&transcript_path);
}
