use delta_model::SessionStatus;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::SendTarget;

/// Composer-first send with no prior session: Delta mints the session id, so
/// the session row (status `spawning`), its `main` thread, and the first
/// prompt's `send` row are all written BEFORE the spawn — the response carries
/// real ids — and once a `UserPromptSubmit` binds the spawn the row activates
/// and the first user line correlates (the turn starts) through the normal
/// machinery.
#[tokio::test]
async fn composer_first_send_writes_real_rows_before_spawn() {
    let ix = interactor();

    // No session exists yet. The send spawns a fresh session and returns the
    // real, already-persisted send row bound to the new session and its main
    // thread.
    let (returned, _) = ix
        .enqueue_send(
            SendTarget::NewSession { workdir: None },
            "first message",
            None,
        )
        .await
        .unwrap();
    assert_ne!(returned.id, 0, "the send row is persisted before the spawn");
    assert_eq!(returned.text, "first message");
    let session_id = returned.session_id.clone();
    assert!(
        !session_id.as_str().is_empty(),
        "a real session id is minted"
    );

    // The eager session row exists as `spawning`: the transcript path is only
    // learned from the first hook, so it is still unset.
    let session = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the session row is written before the spawn");
    assert_eq!(session.status, SessionStatus::Spawning);
    assert_eq!(session.transcript_path, None);
    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    assert_eq!(returned.thread_id, main, "the send targets the main thread");

    // The spawn created exactly one tmux session in its own workdir.
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].name, "delta-1");
    assert_eq!(created[0].workdir, "/work/delta-1");

    // The first prompt rides on the launch command line as a trailing positional
    // argument (claude auto-submits it at startup), NOT injected into the pane
    // after launch. The last argv entry is the prompt, and no send_line keystroke
    // dispatch happened during the fresh spawn.
    assert_eq!(
        created[0].command.last().map(String::as_str),
        Some("first message"),
        "the first prompt is the trailing positional launch argument"
    );
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert!(
        sent.is_empty(),
        "the fresh spawn submits the prompt at launch, not via send_line"
    );

    // Delta pinned the conversation's session id at spawn time; the registry's
    // pending spawn carries the same id the send row was written under.
    assert_eq!(ix.pending_session_ids().await, vec![session_id.clone()]);

    // The first UserPromptSubmit reports that pinned session id. It binds the
    // spawn and activates the row — `spawning` → `active`, filling in the
    // hook-reported transcript path — and the already-written send correlates.
    ix.transcript_fake()
        .push_to("/work/delta-1/t.jsonl", user_line("u-1", "first message"));
    let (events, _) = ix
        .on_user_prompt_submit(submit_in(
            session_id.as_str(),
            "/work/delta-1/t.jsonl",
            "/work/delta-1",
            "first message",
        ))
        .await
        .unwrap();

    // The session activated and the first turn started (the eager send row
    // matched the user line).
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: session_id.clone(),
    }));
    let started = events
        .iter()
        .any(|e| matches!(e, SessionEvent::TurnStarted { .. }));
    assert!(started, "the first prompt correlates into a turn");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "a bound spawn's first prompt is not external input"
    );
    let session = ix.store().session(&session_id).await.unwrap().unwrap();
    assert_eq!(session.status, SessionStatus::Active);
    assert_eq!(
        session.transcript_path.as_deref(),
        Some("/work/delta-1/t.jsonl"),
        "the bind fills the hook-reported transcript path"
    );

    // The user line landed on main and the send is now matched (FIFO clear).
    let view = ix.thread_view(main).await.unwrap();
    assert!(view.iter().any(|m| m.uuid.as_str() == "u-1"));
    assert!(ix
        .store()
        .head_dispatched_send(&session_id)
        .await
        .unwrap()
        .is_none());
}
