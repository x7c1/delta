use delta_model::{Role, SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

/// When the user types a slash command Claude Code does not recognize (e.g.
/// `/review-pr` when no such command exists), Claude rejects it entirely
/// client-side: it fires NO `UserPromptSubmit` echo and NO `Stop` hook, and
/// writes NO `user`/`assistant` line — only a single `type: "system"` /
/// `subtype: "informational"` warning "Unknown command: /review-pr". Yet Delta
/// dispatched the command as a send and moved the turn machine to `AwaitingEcho`.
///
/// Without a transcript-driven fallback this hangs the session: the dispatched
/// send stays outstanding forever, wedging the single-outstanding rule so no
/// later send can dispatch — the user's reported "no response, and nothing after
/// it works" symptom.
///
/// Ingesting the notice must instead: (a) surface the "Unknown command: …"
/// warning as a `Role::System` message (not drop it to an empty line), (b)
/// consume the dispatched send, (c) emit `TurnInterrupted` so the browser clears
/// the stuck pending chip, and (d) return the turn machine to `Idle` so a send
/// composed during the command dispatches without the user sending anything
/// first — mirroring the known-local-command unstick.
///
/// And (e) it must do all that *honestly*: the ingest tells the machine the
/// command's own send was resolved (`TurnInput::CommandResolved`), which is a
/// designed-for end of that send's degenerate turn. Routing it as a generic
/// `Stop` instead landed on the defensive `(AwaitingEcho, Stop)` arm, which
/// reads the same situation as lost keystrokes: one "anomalous turn
/// transition" warning plus one "outstanding send never echoed" warning per
/// resolved command, and a requeue claimed against a row the paired
/// `SendMatched` had just marked. So no requeue may be spent here.
#[tokio::test]
async fn unknown_command_unsticks_turn() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The user runs `/review-pr` (which does not exist): Delta dispatches it (one
    // set of keystrokes) and the turn machine is now `AwaitingEcho` for that send
    // — an unknown command never echoes, so no `on_user_prompt_submit` follows.
    let (command, _) = ix.enqueue_send(to(main), "/review-pr", None).await.unwrap();
    // A follow-up prompt is composed during the command and held queued behind
    // the outstanding `/review-pr` send.
    let (follow_up, _) = ix
        .enqueue_send(to(main), "now do something valid", None)
        .await
        .unwrap();
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "the follow-up send is held queued while the unknown command is outstanding"
    );

    // Claude writes the lone unknown-command notice; the background tail ingests
    // it. No `Stop`/echo hook fires.
    ix.transcript_fake()
        .push(unknown_command_notice_line("notice", "/review-pr"));
    let (_groups, events) = ix.poll_transcript().await.unwrap();

    // (a) The notice surfaces as a system message carrying the warning text.
    let view = ix.thread_view(main).await.unwrap();
    let notice = view
        .iter()
        .find(|m| m.uuid.as_str() == "notice")
        .expect("the unknown-command notice is ingested");
    assert_eq!(notice.role, Role::System);
    assert!(
        notice
            .content_text
            .as_deref()
            .is_some_and(|text| text.contains("Unknown command: /review-pr")),
        "the notice's content must be surfaced, not dropped, got {:?}",
        notice.content_text
    );

    // (c) A `TurnInterrupted` is emitted so the browser clears the stuck chip.
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnInterrupted { session_id, .. } if *session_id == session
        )),
        "ingesting the unknown-command notice emits TurnInterrupted, got {events:?}"
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
        "the queued follow-up dispatches once the unknown-command notice is tailed"
    );
    assert_eq!(second.as_deref(), Some("now do something valid"));
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
        .expect("the follow-up is the lone dispatched send after the unknown command ends");
    assert_eq!(head.text, "now do something valid");
    assert_eq!(head.status, SendStatus::Dispatched);

    // (e) The turn machine moved on to the follow-up's own wait — so the
    // command's turn really did end — and neither send spent a requeue: the
    // resolution is not the "keystrokes were lost" story the defensive `Stop`
    // arm tells, so nothing was warned about and nothing was re-typed.
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho {
            send_id: follow_up.id
        },
        "the command's turn ended and the follow-up's began"
    );
    let (command_requeues, follow_up_requeues) = {
        let (command_id, follow_up_id) = (command.id, follow_up.id);
        ix.with_runtime(&session, move |state| {
            (
                state.requeues_spent(command_id),
                state.requeues_spent(follow_up_id),
            )
        })
        .await
    };
    assert_eq!(
        (command_requeues, follow_up_requeues),
        (0, 0),
        "resolving a slash command claims no requeue: its send was delivered, \
         not swallowed"
    );
}
