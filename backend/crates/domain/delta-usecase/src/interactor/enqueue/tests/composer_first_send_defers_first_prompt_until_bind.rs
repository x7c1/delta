use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::SendTarget;

/// Composer-first send with no prior session: it spawns a fresh session,
/// defers the first prompt, and once a `UserPromptSubmit` binds the spawn the
/// deferred `pending_send` is written and the first user line correlates (the
/// turn starts) through the normal machinery.
#[tokio::test]
async fn composer_first_send_defers_first_prompt_until_bind() {
    let ix = interactor();

    // No session exists yet. The send spawns a fresh session and returns a
    // synthetic (not-yet-persisted) pending row.
    let returned = ix
        .enqueue_send(
            SendTarget::NewSession { workdir: None },
            "first message",
            None,
        )
        .await
        .unwrap();
    assert_eq!(returned.id, 0, "no row persisted before the spawn binds");
    assert_eq!(returned.text, "first message");

    // The spawn created exactly one tmux session in its own workdir, and no
    // pending_send row was written yet (the session id does not exist).
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

    // Delta pinned the conversation's session id at spawn time; read it back so
    // the hook can carry the exact id (a real hook reports the pinned id).
    let session_id = ix.pending_session_ids().await.remove(0);

    // The first UserPromptSubmit reports that pinned session id. It binds the
    // spawn to the now-known session id, registers the session, and writes the
    // deferred pending_send BEFORE attribution — so the user line correlates.
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

    // The session registered and the first turn started (the deferred send was
    // written and matched the user line).
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: session_id.clone(),
    }));
    let started = events
        .iter()
        .any(|e| matches!(e, SessionEvent::TurnStarted { .. }));
    assert!(started, "the deferred first prompt correlates into a turn");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "a bound deferred send is not external input"
    );

    // The user line landed on main and the send is now matched (FIFO clear).
    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    let view = ix.thread_view(main).await.unwrap();
    assert!(view.iter().any(|m| m.uuid.as_str() == "u-1"));
    assert!(ix
        .store()
        .head_pending_send(&session_id)
        .await
        .unwrap()
        .is_none());
}
