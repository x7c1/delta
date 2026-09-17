use super::listed_state;
use crate::interactor::testing::*;
use crate::SendTarget;

/// The row reports the window in which a launch's pane is up but nothing has
/// bound it — the window a human has to reach the pane in (an interactive
/// prompt the launch stopped on).
///
/// It is on the row, not only on the one-shot `spawn_pane_ready` event, so a
/// browser that was not listening when the pane came up (a reload mid-launch, a
/// second tab) still learns of it: the row is refetched, the event is not
/// replayed.
///
/// Walked across the whole launch, because every neighbouring state must read
/// `pane_starting: false` for the flag to mean anything: still preparing (no
/// pane exists yet — `open: false` too, which is why the row alone could not
/// tell the two apart before), pane up, bound, and closed.
#[tokio::test]
async fn list_sessions_page_reports_a_starting_pane_until_it_binds() {
    let gate = TmuxGate::closed();
    let ix = interactor_with_tmux(FakeTmux::default().with_gate(&gate));

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .expect("the send is accepted, and writes the eager `spawning` row");
    let session_id = send.session_id.clone();

    // Held inside `create_session`: the row is listed and the spawn recorded,
    // but no pane exists yet, so there is nothing to attach to.
    gate.await_entered().await;
    assert_eq!(
        listed_state(&ix, &session_id).await,
        (false, false),
        "a launch still preparing offers no pane"
    );

    // The pane comes up. Still `spawning`, still not open — and now attachable.
    gate.open();
    ix.await_launch().await;
    assert_eq!(
        listed_state(&ix, &session_id).await,
        (false, true),
        "the launched-but-unbound spawn's pane is reported on the row"
    );

    // The first hook binds it: the session is open, and the starting window is
    // over — the row's `open` flag carries the pane from here.
    ix.on_user_prompt_submit(submit_in(
        session_id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hello",
    ))
    .await
    .unwrap();
    assert_eq!(
        listed_state(&ix, &session_id).await,
        (true, false),
        "binding ends the starting window"
    );

    // Closing takes the pane away: the row stays, with neither flag.
    ix.close_session(&session_id).await.unwrap();
    assert_eq!(
        listed_state(&ix, &session_id).await,
        (false, false),
        "a closed session has no pane to attach to"
    );
}
