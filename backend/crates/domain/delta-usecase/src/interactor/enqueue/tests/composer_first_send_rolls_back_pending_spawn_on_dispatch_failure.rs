use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::SendTarget;

/// When the composer-first spawn cannot launch its tmux session
/// (`create_session` fails), the use case surfaces the launch error AND rolls
/// the just-recorded pending spawn out of `pending`, so a later, unrelated
/// `UserPromptSubmit` is not mis-bound to it (it registers as external instead).
#[tokio::test]
async fn composer_first_send_rolls_back_pending_spawn_on_dispatch_failure() {
    use crate::error::Error;

    let ix = interactor_with_failing_create_session();

    // No session yet: the composer-first send records the pending spawn, then
    // fails to launch the tmux session. The error propagates.
    let err = ix
        .enqueue_send(
            SendTarget::NewSession { workdir: None },
            "first message",
            None,
        )
        .await
        .expect_err("a failed session launch must propagate");
    assert!(matches!(err, Error::Tmux(_)));

    // The spawn was rolled back: no pending spawn remains, so a later hook
    // carrying any session id finds none to bind and is treated as an external,
    // closed session rather than binding to the abandoned pane.
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "the failed spawn left no pending entry behind"
    );
    // The eager session row (and its send, by cascade) was rolled back too: the
    // caller got the error synchronously, so nothing must linger in the store.
    assert!(
        ix.store().list_sessions().await.unwrap().is_empty(),
        "the failed spawn left no session row behind"
    );
    let (events, _) = ix
        .on_user_prompt_submit(submit_in(
            "sess-late",
            "/work/delta-1/t.jsonl",
            "/work/delta-1",
            "typed in claude",
        ))
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "no pending spawn remained, so the hook is external input"
    );
    assert!(
        ix.pane_for_session(&SessionId::from("sess-late"))
            .await
            .is_none(),
        "the rolled-back spawn must not bind a later session"
    );
}
