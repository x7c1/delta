use std::time::{Duration, Instant};

use delta_model::SessionId;

use crate::interactor::session_actor::runtime::PENDING_SPAWN_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::{AttachablePane, SendTarget};

/// The PTY bridge can reach a spawn's pane before anything binds it — and
/// nothing else.
///
/// A launch that stops on an interactive prompt (Claude Code's workspace-trust
/// dialog is the one that found this) fires no hook at all, so the session never
/// binds and the only way past it is a human at the pane. The pane exists for
/// that whole time, so the lookup the bridge routes by resolves it, marked
/// unbound: the caller knows not to type into it on Delta's behalf.
///
/// The states it must NOT resolve are the point of the same test: a launch whose
/// pane has not been created yet has nothing to attach to, and a session whose
/// launch failed, or that was closed, has nothing left.
#[tokio::test]
async fn the_pty_lookup_resolves_a_launched_but_unbound_spawn() {
    let gate = TmuxGate::closed();
    let ix = interactor_with_tmux(FakeTmux::default().with_gate(&gate));

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .expect("the send is accepted before the launch runs");
    let session_id = send.session_id.clone();

    // The launch is inside `create_session`: the spawn is recorded (so the
    // first hook has something to bind) but the tmux session does not exist
    // yet. Attaching here would attach to nothing.
    gate.await_entered().await;
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        None,
        "a spawn whose pane has not been created yet offers nothing to attach to"
    );

    // The launch reports back: the pane is up, and nothing has bound it.
    gate.open();
    ix.await_launch().await;
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        Some(AttachablePane {
            pane: "delta-1:0.0".to_owned(),
            bound: false,
        }),
        "a launched spawn's pane is attachable, and reported as not bound"
    );
    assert!(
        !ix.is_session_open(&session_id).await,
        "the session really is still starting — this is not a bound pane"
    );

    // The first hook binds it: the same pane, now bound.
    ix.on_user_prompt_submit(submit_in(
        session_id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hello",
    ))
    .await
    .unwrap();
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        Some(AttachablePane {
            pane: "delta-1:0.0".to_owned(),
            bound: true,
        }),
        "binding does not move the pane, only what may be done to it"
    );

    // Closing it takes the pane away again.
    ix.close_session(&session_id).await.unwrap();
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        None,
        "a closed session has no pane to attach to"
    );
}

/// The failed twin of the above: a spawn whose launch never bound and was reaped
/// resolves nothing, because its pane was killed with it.
#[tokio::test]
async fn the_pty_lookup_resolves_nothing_for_a_spawn_that_failed() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-doomed");

    ix.store()
        .insert_spawning_session(spawning_session(&session_id, "/work"))
        .await
        .unwrap();
    ix.push_pending_spawn_at(
        "delta-1",
        &session_id,
        now - PENDING_SPAWN_DEADLINE - Duration::from_secs(1),
    )
    .await;
    ix.tmux_fake()
        .live
        .lock()
        .unwrap()
        .push("delta-1".to_owned());

    // Attachable right up to the moment the watchdog gives up on it…
    assert!(
        ix.pane_for_session(&session_id).await.is_some(),
        "an unbound spawn is attachable while its pane is alive"
    );

    let events = ix.reap_stale_spawns(now, TICK_BOUND).await.unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionEvent::SpawnFailed { .. })),
        "the sweep reaped the spawn"
    );

    // …and nothing afterwards.
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        None,
        "a failed spawn's pane is gone, so there is nothing to attach to"
    );
}
