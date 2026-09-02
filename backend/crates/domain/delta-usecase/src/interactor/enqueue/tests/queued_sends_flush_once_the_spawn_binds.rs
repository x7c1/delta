//! A send accepted while a Claude session was still spawning is typed once the
//! launch binds — on both bind shapes, and never from inside the hook.
//!
//! Binding is a *blocking* hook: `SessionStart`/`UserPromptSubmit` hold `claude`
//! until Delta's handler returns, so a keystroke typed from inside one lands
//! while the TUI is not accepting input and is silently lost (the same reason
//! `SessionStart(source=resume)` only marks readiness). The bind therefore
//! *posts* a flush to the session's own mailbox instead of dispatching inline,
//! and the two tests here pin the two shapes that reach the queue from there:
//!
//! - a spawn **with** a first prompt binds `AwaitingEcho`, so the posted flush
//!   is a no-op and the queue drains at that turn's `Stop`;
//! - a **prompt-less** spawn binds idle with no `Stop` ever coming, so the
//!   posted flush is the only thing that can move its queue.

use delta_model::SendStatus;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};
use crate::SendTarget;

/// The queue of a spawn that carried a first prompt drains at that prompt's
/// turn end, in order — the ordinary queued path, reached without any special
/// casing at bind.
#[tokio::test]
async fn a_queued_send_flushes_at_the_first_turn_end() {
    let ix = interactor();

    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
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
    // Awaits the launch, so the spawn is recorded and pending (unbound): a send
    // now lands squarely in the spawning window.
    assert_eq!(ix.pending_session_ids().await, vec![session_id.clone()]);

    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    let (queued, _) = ix
        .enqueue_send(to(main), "second message", None)
        .await
        .expect("a plain send to a still-spawning session is accepted");
    assert_eq!(queued.status, SendStatus::Queued);
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "nothing is typed while the session is still spawning"
    );

    // The launch's first prompt auto-submits and binds the spawn. The session
    // is `AwaitingEcho` from here — the first prompt owns the turn — so the
    // queued row must NOT be typed on top of it.
    let (events, _) = ix
        .on_user_prompt_submit(submit_for(
            session_id.as_str(),
            "/work/delta-1/t.jsonl",
            "first message",
        ))
        .await
        .unwrap();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SessionEvent::SendDispatched { .. })),
        "the bind does not dispatch the queue on top of the opening turn"
    );
    // Let the flush the bind posted run, and confirm it changed nothing.
    ix.with_runtime(&session_id, |_| ()).await;
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        0,
        "the posted flush is a no-op while the opening turn is in flight"
    );

    // The opening turn ends: now the queue moves, on the ordinary `Stop` path.
    let events = ix
        .on_stop(StopHook {
            session_id: session_id.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();
    assert!(
        events.iter().any(|event| matches!(
            event,
            SessionEvent::SendDispatched { send_id, .. } if *send_id == queued.id
        )),
        "the turn end promotes the queued send, got {events:?}"
    );
    assert_eq!(
        ix.tmux_fake()
            .sent
            .lock()
            .unwrap()
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<Vec<_>>(),
        vec!["second message".to_owned()],
        "the queued send is typed once, after the first prompt's turn"
    );
}

/// A prompt-less spawn (the cold-start path) binds *idle*, and no `Stop` is
/// coming to flush its queue. The flush the bind posts to the actor's own
/// mailbox is what dispatches it — and, crucially, it runs after the hook
/// handler has returned rather than from inside it.
#[tokio::test]
async fn a_queued_send_flushes_when_a_prompt_less_spawn_binds() {
    let ix = interactor();

    ix.new_session().await.unwrap();
    let ids = ix.pending_session_ids().await;
    let session_id = ids
        .first()
        .expect("the cold start recorded a spawn")
        .clone();

    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    let (queued, _) = ix
        .enqueue_send(to(main), "typed while it was starting", None)
        .await
        .expect("a plain send to a still-spawning session is accepted");
    assert_eq!(queued.status, SendStatus::Queued);

    // `SessionStart(startup)` binds the spawn. The hook blocks `claude` until
    // this returns, so the dispatch must not be part of its work: the handler's
    // own events carry the registration and nothing else.
    let events = ix
        .on_session_start(session_start(session_id.as_str(), "startup"))
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionEvent::SessionRegistered { .. })),
        "the startup hook bound and registered the spawn, got {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SessionEvent::SendDispatched { .. })),
        "no keystroke is typed from inside the blocking hook, so the hook \
         reports no dispatch: it posts a flush instead"
    );

    // Let the posted flush run on the next mailbox iteration.
    ix.with_runtime(&session_id, |_| ()).await;

    assert_eq!(
        ix.tmux_fake()
            .sent
            .lock()
            .unwrap()
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<Vec<_>>(),
        vec!["typed while it was starting".to_owned()],
        "the queued send is typed once the bind's flush runs"
    );
    assert_eq!(
        ix.store().send(queued.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the flushed row was promoted, so its echo can correlate normally"
    );
}
