use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::open_sessions::RESUME_DISPATCH_SETTLE;

/// `enqueue_send` against a known-but-*closed* session resumes it as part of the
/// send (the documented "Closed" branch): `ensure_open` finds no live pane, so
/// it spawns `claude --resume <id>` and then dispatches the message into the
/// freshly-resumed pane on the normal path — all within the single
/// `enqueue_send` call, with no prior explicit `open_session`. This pins the
/// resume-within-send wiring, which the test above only exercises after a
/// separate `open_session` (the already-open branch of `ensure_open`).
#[tokio::test]
async fn enqueue_send_resumes_a_closed_session_then_dispatches() {
    let ix = interactor();
    // Register a known-but-closed session (an external claude in /elsewhere):
    // it has a store row but no live pane.
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");

    let main = ix.store().main_thread_id(&id).await.unwrap();
    let pending = ix
        .enqueue_send(to(main), "after resume", None)
        .await
        .unwrap();
    assert_ne!(pending.id, 0, "a real pending_send row was written");

    // The send resumed the session: a `claude --resume sess-R` spawn was
    // recorded in the stored cwd, with no prior explicit open_session call.
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    let resume = created
        .iter()
        .find(|c| c.command.iter().any(|a| a == "--resume"))
        .expect("the send resumed the closed session");
    assert_eq!(
        resume.command,
        vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            TEST_SETTINGS_PATH.to_owned(),
            "--resume".to_owned(),
            "sess-R".to_owned()
        ],
    );
    assert_eq!(resume.workdir, "/elsewhere", "resumes in the stored cwd");

    // The session is open but resumed-but-not-ready, so the first prompt's
    // keystroke is held — not dispatched within the `enqueue_send` call.
    let pane = ix.pane_for_session(&id).await.expect("now open after send");
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "the resume's first prompt is held until SessionStart(resume)"
    );

    // SessionStart(source=resume) only marks the resume ready (the hook blocks
    // `claude`, so it must not type the keystroke itself): nothing is dispatched
    // from the handler.
    ix.on_session_start(session_start("sess-R", "resume"))
        .await
        .unwrap();
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "the readiness hook only marks ready; it does not dispatch from the handler"
    );

    // On the background tick, once the resume has settled, the held prompt is
    // dispatched into the resumed pane on the normal `send_line` path.
    ix.dispatch_ready_resumes(Instant::now() + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert!(
        sent.iter().any(|(p, t)| p == &pane && t == "after resume"),
        "the held first prompt dispatched into the resumed pane on the settle tick"
    );
}
