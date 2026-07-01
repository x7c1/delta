use delta_model::{Role, SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// A slash/local command (e.g. `/review-pr`) is handled by Claude entirely
/// client-side: it fires NO `UserPromptSubmit` echo and NO `Stop` hook, yet
/// Delta dispatches it as a send and moves the turn machine to `AwaitingEcho`.
/// Claude records the command as a group of `type: "user"` lines sharing one
/// `promptId` — a `<local-command-caveat>` it flags `isMeta`, the bare
/// command-name line, then the command's `<local-command-stdout>` — only the
/// caveat being `isMeta`.
///
/// Without a transcript-driven fallback this produced two bugs: (1) the
/// command-name and stdout lines rendered as USER bubbles in the conversation
/// pane, and (2) the dispatched send stayed outstanding forever, wedging the
/// single-outstanding rule so no later send could dispatch.
///
/// Ingesting the group must instead: (a) fold the command-name and stdout lines
/// to `Role::Meta` (so they collapse, not render as user turns), (b) consume the
/// dispatched send, (c) emit `TurnInterrupted` so the browser clears the stuck
/// pending chip, and (d) return the turn machine to `Idle` so a send composed
/// during the command dispatches without the user sending anything first.
#[tokio::test]
async fn local_command_unsticks_turn_and_folds_to_meta() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The user runs `/review-pr`: Delta dispatches it (one set of keystrokes)
    // and the turn machine is now `AwaitingEcho` for that send — a local command
    // never echoes, so no `on_user_prompt_submit` follows.
    ix.enqueue_send(to(main), "/review-pr", None).await.unwrap();
    // A follow-up prompt is composed during the command and held queued behind
    // the outstanding `/review-pr` send.
    ix.enqueue_send(to(main), "now actually review it", None)
        .await
        .unwrap();
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "the follow-up send is held queued while the local command is outstanding"
    );

    // Claude writes the local-command group (caveat + bare command-name +
    // stdout), all sharing one promptId; the background tail ingests it. No
    // `Stop`/echo hook fires.
    ix.transcript_fake()
        .push(local_command_caveat_line("caveat", "pcmd"));
    ix.transcript_fake()
        .push(local_command_name_line("cmdname", "pcmd", "/review-pr"));
    ix.transcript_fake()
        .push(local_command_stdout_line("stdout", "pcmd"));
    let (_groups, events) = ix.poll_transcript().await.unwrap();

    // (a) The command-name and stdout lines fold to `meta` — not user bubbles.
    let view = ix.thread_view(main).await.unwrap();
    let role_of = |uuid: &str| {
        view.iter()
            .find(|m| m.uuid.as_str() == uuid)
            .map(|m| m.role)
    };
    assert_eq!(role_of("caveat"), Some(Role::Meta));
    assert_eq!(
        role_of("cmdname"),
        Some(Role::Meta),
        "the bare command-name line must fold to meta, not render as a user turn"
    );
    assert_eq!(
        role_of("stdout"),
        Some(Role::Meta),
        "the command's stdout must fold to meta, not render as a user turn"
    );

    // (c) A `TurnInterrupted` is emitted so the browser clears the stuck chip.
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnInterrupted { session_id, .. } if *session_id == session
        )),
        "ingesting the local command emits TurnInterrupted, got {events:?}"
    );

    // (b) + (d) The `/review-pr` send was consumed and the turn returned to idle,
    // so the held follow-up dispatched (its keystrokes were typed) and no send
    // remains queued or stuck dispatched as `/review-pr`.
    let (count, second) = {
        let sent = ix.tmux_fake().sent.lock().unwrap();
        (sent.len(), sent.get(1).map(|p| p.1.clone()))
    };
    assert_eq!(
        count, 2,
        "the queued follow-up dispatches once the local command is tailed"
    );
    assert_eq!(second.as_deref(), Some("now actually review it"));
    assert!(
        ix.store()
            .next_queued_send(&session)
            .await
            .unwrap()
            .is_none(),
        "no send remains queued: the follow-up left `queued` and dispatched"
    );
    // The only outstanding send now is the follow-up; `/review-pr` was matched,
    // not left dangling.
    let head = ix
        .store()
        .head_dispatched_send(&session)
        .await
        .unwrap()
        .expect("the follow-up is the lone dispatched send after the local command ends");
    assert_eq!(head.text, "now actually review it");
    assert_eq!(head.status, SendStatus::Dispatched);
}
