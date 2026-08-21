//! The `/comms` stream over the real stack: a browser joining a live session
//! replays the frames that already flew and then tails the next one.

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::json;

use crate::support::{await_turn_completion, build_app, post_json, ScenarioGuard, REPLY, TIMEOUT};

/// The comms-log stream, over the same real stack: a browser joining a live
/// session receives the frames that already flew and then the next live one.
///
/// This is the endpoint's contract asserted end to end — the frames come from a
/// real adapter driving a real `fake-codex` over a real turn, and they are read
/// through the exact subscription the `/comms` route pumps into its socket. Only
/// the WebSocket bytes are left out (the handler does nothing but serialize each
/// frame and write it), which keeps the test free of a WebSocket client
/// dependency without weakening what it proves.
#[tokio::test(flavor = "multi_thread")]
async fn the_comms_log_replays_a_live_sessions_frames_then_tails_new_ones() {
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_comms_loop",
            "turn": {{
                "turn_id": "turn_comms_loop",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_completed", "item": {{ "id": "item_1", "type": "agentMessage", "text": "{REPLY}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    ));

    let (app, state) = build_app(&scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // A first turn runs to completion, so by the time we look there is history.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "hello codex" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();
    let thread_id = body["send"]["thread_id"]
        .as_i64()
        .expect("the send response carries its main thread id");
    await_turn_completion(&mut events).await;

    // Now the browser opens the pane — mid-session, after the frames flew.
    let mut watcher = state.watch_comms_log(&session_id);

    // The replay: the session's own launch first, then the turn's pushed flow,
    // strictly ordered. Draining to the end of the buffer (rather than reading a
    // fixed count) is what makes the assertion independent of how many frames the
    // scenario happens to emit.
    let replayed = drain_buffered_comms(&mut watcher).await;
    let methods: Vec<Option<&str>> = replayed
        .iter()
        .map(|frame| frame.method.as_deref())
        .collect();
    assert_eq!(
        methods.first().copied().flatten(),
        Some("thread/start"),
        "the replay starts at the session's launch: {methods:?}"
    );
    assert!(
        methods.contains(&Some("turn/completed")),
        "the replay includes the completed turn's pushed frame: {methods:?}"
    );
    let seqs: Vec<u64> = replayed.iter().map(|frame| frame.seq).collect();
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "replayed frames are strictly ordered: {seqs:?}"
    );
    let last_replayed_seq = *seqs.last().expect("the replay is non-empty");

    // And then the live tail: a second prompt on the same session must show up on
    // the SAME subscription, numbered after the replay — the handoff a client
    // connecting mid-session depends on.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "thread_id": thread_id, "text": "second message" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "the second send: {body:?}");

    let live = tokio::time::timeout(TIMEOUT, async {
        loop {
            let frame = watcher.next().await.expect("the stream stayed open");
            if frame.method.as_deref() == Some("turn/start") {
                return frame;
            }
        }
    })
    .await
    .expect("timed out waiting for the second turn's frame on the live tail");
    assert!(
        live.seq > last_replayed_seq,
        "the live frame continues the replay's numbering ({} > {last_replayed_seq})",
        live.seq
    );
    assert_eq!(live.direction, delta_wire::WireCommsDirection::ToAgent);
    assert_eq!(live.kind, delta_wire::WireCommsFrameKind::Request);
}

/// A session with no adapter behind it (never launched, so nothing was ever
/// recorded) gets an open, quiet stream rather than an error — the pane shows its
/// idle state, which is the honest answer for "nothing is being exchanged".
#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_session_gets_an_idle_stream_rather_than_a_failure() {
    let scenario = ScenarioGuard::write(r#"{ "thread_id": "thr_idle" }"#);
    let (_app, state) = build_app(&scenario);

    let mut watcher = state.watch_comms_log("sess-never-launched");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), watcher.next())
            .await
            .is_err(),
        "the stream is open and simply has nothing to say"
    );
}

/// Read the frames a fresh subscription already had buffered, stopping when it
/// goes quiet (there is no in-band "end of replay" marker — by design, since the
/// stream is one continuous sequence).
async fn drain_buffered_comms(
    watcher: &mut delta_server::CommsSubscription,
) -> Vec<delta_wire::WireCommsFrame> {
    let mut frames = Vec::new();
    while let Ok(Some(frame)) =
        tokio::time::timeout(Duration::from_millis(200), watcher.next()).await
    {
        frames.push(frame);
    }
    frames
}
