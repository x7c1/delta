use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// An auto-approved tool resolves its permission request the moment the
/// correlated `tool_result` is ingested: the request is keyed by `tool_use_id`,
/// so ingesting the result yields a `PermissionResolved` for that request, while
/// a non-matching `tool_result` resolves nothing.
#[tokio::test]
async fn ingesting_tool_result_resolves_the_correlated_permission() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    // Bind a live pane so the tail (scoped to open sessions) polls `sess-1`.
    ix.bind_open_session("delta-1", &SessionId::from("sess-1"))
        .await;

    // A permission request is recorded for an imminent tool call. PreToolUse
    // only records it (the browser notice is emitted by the `PermissionRequest`
    // hook), so the request id is read back from the store.
    ix.on_pre_tool_use(
        &SessionId::from("sess-1"),
        "Bash",
        r#"{"command":"ls"}"#,
        "toolu_01",
    )
    .await
    .unwrap();
    let request_id = {
        let g = ix.store().inner.lock().unwrap();
        g.permissions
            .iter()
            .find(|r| r.tool_use_id == "toolu_01")
            .expect("the request is recorded")
            .id
    };

    // A tool_result for a *different* tool_use_id resolves nothing.
    ix.transcript_fake()
        .push(tool_result_line("r-other", "toolu_other"));
    let (_groups, events) = ix.poll_transcript().await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::PermissionResolved { .. })),
        "a non-matching tool_result must not resolve the request"
    );

    // The correlated tool_result resolves the open request and emits
    // `PermissionResolved` for exactly that request.
    ix.transcript_fake()
        .push(tool_result_line("r-1", "toolu_01"));
    let (_groups, events) = ix.poll_transcript().await.unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::PermissionResolved { request_id: id, .. } if *id == request_id
        )),
        "the matching tool_result resolves the request"
    );

    // A re-ingested tool_result (no longer pending) resolves nothing again.
    ix.transcript_fake()
        .push(tool_result_line("r-1-dup", "toolu_01"));
    let (_groups, events) = ix.poll_transcript().await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::PermissionResolved { .. })),
        "an already-resolved request is not resolved twice"
    );
}
