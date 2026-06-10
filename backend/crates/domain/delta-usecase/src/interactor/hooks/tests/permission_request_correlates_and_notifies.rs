use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

#[tokio::test]
async fn permission_request_correlates_to_the_recorded_request() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // PreToolUse records the request (the interactive dialog has not appeared
    // yet); grab the recorded row's id from the fake store.
    ix.on_pre_tool_use(&session, "Bash", r#"{"command":"ls"}"#, "toolu_01")
        .await
        .unwrap();
    let recorded_id = {
        let g = ix.store().inner.lock().unwrap();
        g.permissions
            .iter()
            .find(|r| r.tool_use_id == "toolu_01")
            .expect("the request is recorded")
            .id
    };

    // The PermissionRequest hook (exact tool_input) emits the notice for that row.
    let events = ix
        .on_permission_request(&session, "Bash", r#"{"command":"ls"}"#)
        .await
        .unwrap();
    match events.as_slice() {
        [SessionEvent::PermissionRequested {
            request_id,
            tool_name,
            ..
        }] => {
            assert_eq!(*request_id, recorded_id);
            assert_eq!(tool_name, "Bash");
        }
        other => panic!("expected a single PermissionRequested, got {other:?}"),
    }
}

#[tokio::test]
async fn permission_request_falls_back_to_the_latest_pending_for_the_tool() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // A pending request exists for (session, Bash), but with a different
    // tool_input than the one the PermissionRequest hook reports.
    ix.on_pre_tool_use(&session, "Bash", r#"{"command":"ls"}"#, "toolu_01")
        .await
        .unwrap();
    let recorded_id = {
        let g = ix.store().inner.lock().unwrap();
        g.permissions
            .iter()
            .find(|r| r.tool_use_id == "toolu_01")
            .expect("the request is recorded")
            .id
    };

    // Without an exact tool_input match it still correlates to the latest
    // pending request for that (session, tool_name).
    let events = ix
        .on_permission_request(&session, "Bash", r#"{"command":"pwd"}"#)
        .await
        .unwrap();
    match events.as_slice() {
        [SessionEvent::PermissionRequested { request_id, .. }] => {
            assert_eq!(*request_id, recorded_id);
        }
        other => panic!("expected a single PermissionRequested, got {other:?}"),
    }
}
