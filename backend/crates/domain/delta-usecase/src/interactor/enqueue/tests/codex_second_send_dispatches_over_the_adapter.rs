use delta_model::{AgentProvider, MessageUuid, SendStatus};

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
/// turn as consuming no send (`InFlight { send_id: None }`), (d) completes its
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

    // The turn is tracked as consuming no send: in flight with no send id, so a
    // later `TurnCompleted → Stop` cannot cancel this successful send.
    assert_eq!(
        ix.live_state_for(&session_id).await.turn,
        TurnState::InFlight { send_id: None },
        "the second turn is tracked as consuming no send, like the first"
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

/// "Branch from selected text" works on a Codex session: it is enabled by
/// hidden-context injection (`ContextInjectionCapability::HiddenPerTurn` via
/// `thread/inject_items`), NOT by native fork, so it is no longer rejected.
///
/// A branch send must (a) create the SAME delta-side branch structure Claude
/// builds — a new thread lane with the branched-from message as its semantic
/// parent, via the shared `resolve_branch_target` — (b) deliver the selected
/// passage as hidden context over the adapter's `inject_context` BEFORE the
/// turn dispatches, and (c) dispatch the branch turn over the same adapter send
/// path as any other Codex turn. This is the regression proof that Codex branch
/// send routes through inject-context + shared branch bookkeeping rather than
/// the removed `ForkCapability::None` rejection.
#[tokio::test]
async fn codex_branch_send_injects_context_and_reuses_branch_bookkeeping() {
    let factory = FakeAgentFactory::new("thr_branch", Some("turn_2"));
    let ix = interactor_with_codex_factory(factory.clone());

    // Turn 1: open the Codex session with a first prompt, then let it settle to
    // idle — the state a real branch send arrives in.
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
    let main_thread = first.thread_id;
    ix.apply_turn_input(&session_id, TurnInput::Stop)
        .await
        .unwrap();

    // Branch from a selected passage of an earlier message.
    let parent = MessageUuid::from("uuid-parent");
    let quote = "the selected passage";
    let (branch, events) = ix
        .enqueue_send(branch_off(main_thread, &parent), "branch text", Some(quote))
        .await
        .expect("the Codex branch send is accepted (no ForkCapability rejection)");

    // (a) Same delta-side branch structure Claude builds: a new thread lane with
    // the branched-from message as its semantic parent, parented to the source
    // thread and rooted at the branched-from message.
    assert_eq!(branch.session_id, session_id, "same session");
    assert_ne!(
        branch.thread_id, main_thread,
        "the branch send lands on a new thread lane, not the source thread"
    );
    assert_eq!(
        branch.semantic_parent_uuid,
        Some(parent.clone()),
        "the branch send carries the branched-from message as its semantic parent"
    );
    let child = ix.store().thread(branch.thread_id).await.unwrap().unwrap();
    assert_eq!(
        child.parent_thread_id,
        Some(main_thread),
        "the branch child thread is parented to the source thread"
    );
    assert_eq!(
        child.root_message_uuid,
        Some(parent),
        "the branch child thread is rooted at the branched-from message"
    );
    assert_eq!(
        child.title, quote,
        "the branch child is titled provisionally from the selected passage"
    );

    // (b) The selected passage was delivered as hidden context over the adapter
    // (the real Codex `thread/inject_items` path), exactly once.
    assert_eq!(
        factory.log().lock().unwrap().injects,
        vec![quote.to_owned()],
        "the branched-from passage was injected as hidden context before dispatch"
    );

    // (c) The branch turn dispatched over the same adapter send path as any
    // other Codex turn — both the opening prompt and the branch prompt reached
    // the adapter, in order.
    assert_eq!(
        factory.log().lock().unwrap().sends,
        vec!["first message".to_owned(), "branch text".to_owned()],
        "the branch turn dispatched over the adapter after the opening turn"
    );

    // The branch turn is tracked as consuming no send and its send row completes
    // at the `turn/start` acknowledgement, like every Codex turn.
    assert_eq!(
        ix.live_state_for(&session_id).await.turn,
        TurnState::InFlight { send_id: None },
        "the branch turn is tracked as consuming no send"
    );
    assert_eq!(
        ix.store().send(branch.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
        "the branch send completes at the turn/start ack"
    );

    // A Codex dispatch produces no synchronous `SessionEvent`s: the branch
    // turn's frames arrive asynchronously over the running pump.
    assert!(
        events.is_empty(),
        "the adapter dispatch returns no synchronous session events"
    );
}
