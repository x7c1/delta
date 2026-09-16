use crate::interactor::testing::*;
use crate::SendTarget;

/// Attaching to a pane that has not bound yet types nothing into it.
///
/// The PTY bridge wipes residual input before a fresh attach, because a previous
/// client's detach leaves a focus-out report that Claude renders as a stray
/// blank line. That wipe is keystrokes — `C-u` and a run of backspaces — and it
/// is only safe against a bound pane, which is known to be an agent sitting at
/// its prompt. An unbound spawn's pane may be showing anything, up to and
/// including a dialog whose options those very keys would answer, so the wipe
/// must not reach it: sending it would pick an answer on the user's behalf, in
/// a pane they opened precisely in order to answer it themselves.
///
/// There is nothing to wipe there either: nothing has ever typed into that pane
/// and no client has detached from it.
#[tokio::test]
async fn attaching_to_an_unbound_pane_sends_it_no_input_clearing_keys() {
    let ix = interactor();

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
        .expect("the send is accepted before the launch runs");
    let session_id = send.session_id.clone();
    ix.await_launch().await;

    // The state under test: the pane is up and attachable, and nothing has
    // bound it.
    let attached = ix
        .attach_pane(&session_id)
        .await
        .expect("a launched spawn's pane is attachable");
    assert!(!attached.bound, "nothing has bound this pane yet");

    // What the bridge does on an attach. The unbound pane is left alone — both
    // by the caller, which skips the clear for an unbound attach, and by the
    // use case itself, which has no bound pane to clear.
    ix.clear_session_input(&session_id).await.unwrap();
    assert!(
        ix.tmux_fake().cleared.lock().unwrap().is_empty(),
        "no keystrokes were sent into the pane of a session that has not bound"
    );
    assert!(
        ix.tmux_fake().pane_input.lock().unwrap().is_empty(),
        "and nothing else was typed into it either"
    );

    // Once the launch binds, the same call clears as it always has.
    ix.on_user_prompt_submit(submit_in(
        session_id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hello",
    ))
    .await
    .unwrap();
    ix.clear_session_input(&session_id).await.unwrap();
    assert_eq!(
        ix.tmux_fake().cleared.lock().unwrap().clone(),
        vec!["delta-1:0.0".to_owned()],
        "a bound pane is cleared before its attach, unchanged"
    );
}
