use delta_model::{Role, SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// Repro of the post-compact hidden-queued-prompt bug at the ingestion layer:
/// while an auto- or manual `/compact` is running, the user submits a prompt;
/// the CLI buffers it in its internal input queue and, once compact finishes,
/// replays it as a plain `type: "user"` line stamped `promptSource: "queued"`.
/// Because Claude Code opens the post-compact turn on ONE `promptId`, the
/// replay shares the compact group's `promptId` alongside the `/compact`
/// caveat/command-name/stdout members.
///
/// Before the fix, the fold folded the queued replay to `Role::Meta` under
/// the group's promptId, matched it against the outstanding send as if it
/// were the group's bare command-name line, and emitted
/// `Effect::LocalCommandTurnEnded` — which the sync interactor turned into
/// `SessionEvent::TurnInterrupted`. Symptom: delta hid the user bubble AND
/// fired a spurious "turn interrupted" toast while the model's real reply
/// for the prompt streamed in.
///
/// After the fix, `is_queued_replay` excludes the replay from the local-command
/// group's Meta reclassification. The ingest must (1) persist the replay as
/// `Role::User` on the send's thread, (2) mark the send row `matched`, and
/// (3) NOT emit `SessionEvent::TurnInterrupted`. The compact-summary line's
/// `Effect::AutoCompactFinished` still fires — that is the sibling recovery
/// path pinned by `compact_summary_redispatches_stuck_dispatched` and its
/// debounce companion, which continue to pass unmodified.
#[tokio::test]
async fn queued_replay_after_compact_matches_the_send_and_does_not_interrupt() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The user submits a prompt that dispatches immediately (idle open session).
    let (send, _) = ix
        .enqueue_send(to(main), "the user's actual prompt", None)
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);

    // Auto- or manual `/compact` runs while the send is still `Dispatched`;
    // Claude writes the compact group as five attributable lines all sharing
    // one `promptId` (the CLI reuses the current promptId across the whole
    // post-compact turn): the compact summary, the `<local-command-caveat>`,
    // the bare `/compact` command-name line, the `<local-command-stdout>`,
    // and the buffered-queue replay of the user's prompt.
    const PCOMPACT: &str = "pcompact";
    ix.transcript_fake().push(with_prompt_id(
        PCOMPACT,
        compact_summary_line("cs-1", "<summary>of the previous conversation</summary>"),
    ));
    ix.transcript_fake()
        .push(local_command_caveat_line("caveat", PCOMPACT));
    ix.transcript_fake()
        .push(local_command_name_line("cmdname", PCOMPACT, "/compact"));
    ix.transcript_fake()
        .push(local_command_stdout_line("stdout", PCOMPACT));
    ix.transcript_fake().push(with_prompt_id(
        PCOMPACT,
        queued_replay_line("u-replay", "the user's actual prompt"),
    ));

    let (_groups, events) = ix.poll_transcript().await.unwrap();

    // (1) The queued replay is persisted as `Role::User` on the send's thread,
    // not swallowed as `Role::Meta`. The regression pin for the hidden
    // user-bubble symptom.
    let view = ix.thread_view(main).await.unwrap();
    let replay = view
        .iter()
        .find(|m| m.uuid.as_str() == "u-replay")
        .expect("the queued replay must be persisted on the main thread");
    assert_eq!(
        replay.role,
        Role::User,
        "the queued replay must persist as User, not Meta — folding it to Meta \
         hides the human prompt from the conversation pane"
    );

    // (2) The send row was marked `matched` by the queued replay — the fold
    // consumed the outstanding `Dispatched` send as a normal echo match, so
    // the pending chip clears through the usual `SendMatched` flow rather
    // than a stuck `Dispatched` row.
    let refreshed = ix.store().send(send.id).await.unwrap().unwrap();
    assert_eq!(
        refreshed.status,
        SendStatus::Matched,
        "the queued replay matches the outstanding send, so the send row \
         leaves `Dispatched` for `Matched` — no lingering pending chip"
    );

    // (3) NO `TurnInterrupted` event fires. This is the pre-fix symptom:
    // `LocalCommandTurnEnded` used to be emitted against the replay (mistaken
    // for the group's command-name line) and the sync interactor lifted it
    // to `SessionEvent::TurnInterrupted`, tearing the live turn down as an
    // interrupt while the model's real reply was still streaming in.
    assert!(
        !events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnInterrupted { session_id: sid, .. } if *sid == session
        )),
        "a queued-replay match must NOT emit TurnInterrupted — the turn is a \
         genuine human-echo match, not a swallowed local command. Events: {events:?}"
    );

    // Sanity: the compact group's OTHER members still fold as command machinery
    // — the fix targets only the queued replay, not the whole group.
    let role_of = |uuid: &str| {
        view.iter()
            .find(|m| m.uuid.as_str() == uuid)
            .map(|m| m.role)
    };
    assert_eq!(role_of("cs-1"), Some(Role::CompactSummary));
    assert_eq!(role_of("caveat"), Some(Role::Meta));
    assert_eq!(role_of("cmdname"), Some(Role::Meta));
    assert_eq!(role_of("stdout"), Some(Role::Meta));
}
