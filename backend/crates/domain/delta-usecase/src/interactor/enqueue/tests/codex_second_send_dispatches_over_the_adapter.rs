use delta_model::{AgentProvider, MessageUuid, SendStatus};

use crate::error::Error;
use crate::interactor::testing::*;
use crate::turn::{TurnInput, TurnState};
use crate::SendTarget;

/// A second (and later) message to an existing Codex session dispatches over the
/// bound adapter — the same path the opening turn takes — rather than Claude's
/// `ensure_open()` → `open_session()` (`claude --resume`) path.
///
/// This is the actor-level regression proof for the dogfooding bug: a Codex
/// session has no tmux pane and no resumable transcript, so a subsequent send
/// that fell through to the Claude resume path failed with `ResumeUnavailable`.
/// The fix branches `enqueue_to_thread` on the non-destructive `open_agent()`
/// accessor and dispatches through the adapter.
///
/// Asserts the second send (a) reaches the adapter (`factory.log().sends` records
/// both prompts), (b) is written against the thread it targeted, (c) tracks the
/// turn `ExternalPrompt`-style (`InFlight { send_id: None }`), (d) completes its
/// send row at the `turn/start` acknowledgement (`Matched`), and (e) produces no
/// synchronous `SessionEvent`s (its frames arrive later over the running pump).
#[tokio::test]
async fn codex_second_send_dispatches_over_the_adapter() {
    let factory = FakeAgentFactory::new("thr_multi", Some("turn_2"));
    let ix = interactor_with_codex_factory(factory.clone());

    // Turn 1: open the Codex session with a first prompt.
    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "first message",
            None,
        )
        .await
        .unwrap();
    let session_id = first.session_id.clone();
    let thread_id = first.thread_id;

    // Complete turn 1 so the session is idle before the second send (the state
    // a real subsequent send arrives in).
    let next = ix
        .apply_turn_input(&session_id, TurnInput::Stop)
        .await
        .unwrap();
    assert_eq!(next, TurnState::Idle, "turn 1 returns to idle");

    // Turn 2: a plain send into the same session's thread. Before the fix this
    // routed through `ensure_open()`/`open_session()` and failed; here it must
    // succeed by dispatching over the adapter.
    let (second, events) = ix
        .enqueue_send(
            SendTarget::Thread {
                thread_id,
                branch_from: None,
            },
            "second message",
            None,
        )
        .await
        .expect("the second send dispatches over the adapter, not a Claude resume");

    // It is the same session, and the send row names the thread it targeted.
    assert_eq!(second.session_id, session_id, "same session");
    assert_eq!(
        second.thread_id, thread_id,
        "the second send is written against the thread it targeted"
    );

    // The turn is tracked ExternalPrompt-style: in flight with no send id, so a
    // later `TurnCompleted → Stop` cannot cancel this successful send.
    assert_eq!(
        ix.live_state_for(&session_id).await.turn,
        TurnState::InFlight { send_id: None },
        "the second turn is tracked ExternalPrompt-style, like the first"
    );

    // The send row was completed at the `turn/start` acknowledgement, not by an
    // echo — it leaves the open/`dispatched` set immediately.
    assert_eq!(
        ix.store().send(second.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
        "the second send is completed at the turn/start ack"
    );

    // Both prompts reached the adapter over the trait, in order — proof the
    // second turn took the adapter path, not `ensure_open()`.
    assert_eq!(
        factory.log().lock().unwrap().sends,
        vec!["first message".to_owned(), "second message".to_owned()],
        "both the opening and the subsequent prompt were dispatched over the adapter"
    );

    // A Codex dispatch produces no synchronous `SessionEvent`s: the turn's
    // frames arrive asynchronously over the already-running event pump.
    assert!(
        events.is_empty(),
        "the adapter dispatch returns no synchronous session events"
    );
}

/// Codex is `ForkCapability::None`, so a branch send into a Codex session is
/// rejected cleanly rather than silently degraded to a plain send. The UI must
/// not offer branching for a no-fork provider, so this is a guard against a
/// caller that ignores that — it returns a clear error and dispatches nothing.
#[tokio::test]
async fn codex_branch_send_is_rejected() {
    let factory = FakeAgentFactory::new("thr_nofork", Some("turn_1"));
    let ix = interactor_with_codex_factory(factory.clone());

    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "first message",
            None,
        )
        .await
        .unwrap();
    let session_id = first.session_id.clone();
    let thread_id = first.thread_id;

    ix.apply_turn_input(&session_id, TurnInput::Stop)
        .await
        .unwrap();

    // A branch send names a message to branch from. Codex cannot fork, so this
    // is rejected before any dispatch.
    let result = ix
        .enqueue_send(
            SendTarget::Thread {
                thread_id,
                branch_from: Some(MessageUuid::from("some-message".to_owned())),
            },
            "branch off this",
            None,
        )
        .await;

    match result {
        Err(Error::Agent(msg)) => assert!(
            msg.contains("branching is not supported"),
            "the error explains branching is unsupported for Codex, got: {msg}"
        ),
        other => panic!("expected a clean Agent error rejecting the branch, got {other:?}"),
    }

    // Nothing was dispatched for the rejected branch send: only the opening
    // prompt ever reached the adapter.
    assert_eq!(
        factory.log().lock().unwrap().sends,
        vec!["first message".to_owned()],
        "the rejected branch send dispatched nothing over the adapter"
    );
}
