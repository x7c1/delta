//! The other half of the pane-gone fix: what the user does next.
//!
//! While the session still read as open, every send was typed into the dead
//! pane, failed, and was cancelled with an error — the user's only way out was
//! to notice and press Close, after which the very same send worked. Once the
//! liveness tick closes the session itself, the next send takes the ordinary
//! resume path with no intervention at all.

use std::time::Instant;

use crate::interactor::testing::*;

#[tokio::test]
async fn a_send_after_a_vanished_pane_close_resumes_the_session() {
    let ix = interactor();

    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();

    // The pane goes away without a hook, and the tick closes the session.
    ix.tmux_fake().vanish_session("delta-1");
    ix.reap_stale_spawns(Instant::now(), TICK_BOUND)
        .await
        .unwrap();
    assert!(!ix.is_session_open(&id).await, "closed by the tick");

    // The next send resumes the session rather than failing into a dead pane.
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let (send, _) = ix
        .enqueue_send(to(main), "still here?", None)
        .await
        .unwrap();
    assert_ne!(send.id, 0, "the send was accepted, not cancelled");

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    let resume = created
        .iter()
        .find(|created| created.command.iter().any(|arg| arg == "--resume"))
        .expect("the send resumed the closed session");
    assert_eq!(
        resume.command,
        vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            TEST_SETTINGS_PATH.to_owned(),
            "--resume".to_owned(),
            id.as_str().to_owned(),
        ],
    );
    assert!(
        ix.is_session_open(&id).await,
        "the resume bound a fresh pane"
    );
}
