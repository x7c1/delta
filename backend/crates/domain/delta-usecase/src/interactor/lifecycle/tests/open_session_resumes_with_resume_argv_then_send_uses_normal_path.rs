use delta_model::SessionId;

use crate::interactor::testing::*;

/// `open_session` resumes a closed known session: it spawns `claude --resume
/// <id>` (asserted via the recorded argv) and binds it, but does NOT dispatch
/// the first prompt yet — the prompt is held until the resume's
/// `SessionStart(source=resume)` arrives, then it dispatches via the normal
/// `send_line` path. A second send after readiness dispatches immediately.
#[tokio::test]
async fn open_session_resumes_with_resume_argv_then_send_uses_normal_path() {
    let ix = interactor();
    // Register a known-but-closed session (an external claude in /elsewhere).
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

    ix.open_session(&id).await.unwrap();

    // The resume spawned `claude --resume sess-R` in the session's stored cwd.
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    let resume = created
        .iter()
        .find(|c| c.command.iter().any(|a| a == "--resume"))
        .expect("a resume spawn was recorded");
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
    let pane = ix.pane_for_session(&id).await.expect("now open");

    // A send writes the pending_send (normal path) but its keystroke is held: the
    // pane is resumed-but-not-ready, so nothing is dispatched yet.
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let pending = ix
        .enqueue_send(to(main), "after resume", None)
        .await
        .unwrap();
    assert_ne!(pending.id, 0, "a real pending_send row was written");
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "the first prompt is held until SessionStart(resume), not dispatched yet"
    );

    // SessionStart(source=resume) signals readiness: the held prompt is now typed
    // into the resumed pane on the normal `send_line` path.
    ix.on_session_start(session_start("sess-R", "resume"))
        .await
        .unwrap();
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert!(
        sent.iter().any(|(p, t)| p == &pane && t == "after resume"),
        "the held first prompt dispatched into the resumed pane on readiness"
    );

    // The session is now ready, so a second send dispatches immediately (no gate).
    ix.enqueue_send(to(main), "second send", None).await.unwrap();
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert!(
        sent.iter().any(|(p, t)| p == &pane && t == "second send"),
        "a send after readiness dispatches immediately"
    );
}
