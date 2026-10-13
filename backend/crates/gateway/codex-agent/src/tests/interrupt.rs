//! Interrupting the current turn through the adapter.

use super::*;

/// The adapter captures the current turn's id from the `turn/start` response
/// (`result.turn.id`) and sends it back in `turn/interrupt`'s params
/// (`{threadId, turnId}`) — the reconciled interrupt shape. Also asserts the
/// reconciled `turn/start` `input` array shape rides the wire.
#[tokio::test]
async fn adapter_captures_turn_id_and_sends_it_on_interrupt() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        // launch -> thread/start, answered with the reconciled `{thread:{id}}`.
        let start = server.next_frame().await;
        assert_eq!(start["method"], "thread/start");
        server
            .send(json!({ "id": start["id"], "result": { "thread": { "id": "thr_u" } } }))
            .await;

        // send -> turn/start. The visible prompt rides as a `TextUserInput` array.
        let turn = server.next_frame().await;
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["input"][0]["type"], "text");
        assert_eq!(turn["params"]["input"][0]["text"], "hello");
        // Real `turn/start` returns the started turn under `result.turn`.
        server
            .send(json!({
                "id": turn["id"],
                "result": { "turn": { "id": "turn_u", "status": "inProgress", "items": [] } }
            }))
            .await;

        // interrupt -> turn/interrupt, carrying the tracked turn id.
        let interrupt = server.next_frame().await;
        assert_eq!(interrupt["method"], "turn/interrupt");
        server
            .send(json!({ "id": interrupt["id"], "result": {} }))
            .await;
        interrupt
    });

    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-000000000009".to_owned(),
            workdir: "/tmp/workdir".to_owned(),
            launch_options: Vec::new(),
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    assert_eq!(handle.provider_session_id, "thr_u");

    let receipt = adapter
        .send(
            &handle,
            SendRequest {
                text: "hello".to_owned(),
            },
        )
        .await
        .expect("send");
    assert_eq!(
        receipt.provider_message_id.as_deref(),
        Some("turn_u"),
        "the send receipt carries the started turn id"
    );

    adapter.interrupt(&handle).await.expect("interrupt");

    let interrupt = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");
    assert_eq!(interrupt["params"]["threadId"], "thr_u");
    assert_eq!(
        interrupt["params"]["turnId"], "turn_u",
        "turn/interrupt must reference the tracked turn id"
    );
}

/// Interrupting a session with no turn in flight is a no-op success: there is no
/// turn id to reference, so the adapter does not send an (invalid) `turn/interrupt`
/// the real server would reject for a missing `turnId`.
#[tokio::test]
async fn interrupt_without_an_active_turn_is_a_no_op() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let start = server.next_frame().await;
        assert_eq!(start["method"], "thread/start");
        server
            .send(json!({ "id": start["id"], "result": { "thread": { "id": "thr_idle" } } }))
            .await;
        // Return the server so the connection stays open; assert no further frame
        // (an interrupt RPC) arrives.
        let next = tokio::time::timeout(Duration::from_millis(200), server.next_frame()).await;
        assert!(
            next.is_err(),
            "no turn/interrupt frame is sent when no turn is in flight, got {next:?}"
        );
    });

    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-00000000000a".to_owned(),
            workdir: "/tmp/workdir".to_owned(),
            launch_options: Vec::new(),
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    adapter
        .interrupt(&handle)
        .await
        .expect("interrupt is a no-op");

    server_task.await.unwrap();
}
