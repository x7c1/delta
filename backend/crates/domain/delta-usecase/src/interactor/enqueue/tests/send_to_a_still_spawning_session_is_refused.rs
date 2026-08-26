use crate::interactor::testing::*;
use crate::SendTarget;

/// A session is listed — and so reachable from the browser — from the moment
/// its first send is accepted, while its launch is still coming up. A second
/// send arriving in that window must be refused: no pane is bound to the
/// session yet and its transcript does not exist, so the ordinary
/// closed-session path (`ensure_open` → `claude --resume <id>`) would launch a
/// SECOND agent against a conversation the first launch has not written.
#[tokio::test]
async fn send_to_a_still_spawning_session_is_refused() {
    use crate::error::Error;

    let ix = interactor();

    // The composer's first send spawns the session and returns its real ids.
    // No hook has fired, so the spawn is still pending (unbound).
    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: crate::AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "first message",
            None,
        )
        .await
        .unwrap();
    let session_id = first.session_id.clone();
    assert_eq!(ix.pending_session_ids().await, vec![session_id.clone()]);

    let launches_before = ix.tmux_fake().created.lock().unwrap().len();
    let main = ix.store().main_thread_id(&session_id).await.unwrap();

    let err = ix
        .enqueue_send(to(main), "too early", None)
        .await
        .expect_err("a send to a still-starting session must fail");
    assert!(
        matches!(err, Error::SessionSpawning(ref s) if s == session_id.as_str()),
        "the refusal propagates as SessionSpawning, got: {err:?}"
    );

    // Nothing was launched or typed: no resume ran, and the spawn is untouched
    // so the pending first prompt still binds normally when its hook arrives.
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        launches_before,
        "no second agent was launched"
    );
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "no keystrokes were dispatched"
    );
    assert_eq!(ix.pending_session_ids().await, vec![session_id.clone()]);

    // And no optimistic row was written for the refused send: the only open
    // send is still the first prompt the spawn carries.
    let open = ix.store().open_sends(&session_id).await.unwrap();
    assert_eq!(
        open.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        vec!["first message"],
        "the refused send left no row behind"
    );
}
