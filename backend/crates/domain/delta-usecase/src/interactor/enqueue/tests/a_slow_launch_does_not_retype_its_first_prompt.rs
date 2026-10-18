//! A fresh spawn whose launch takes longer than the echo deadline must not have
//! its first prompt re-typed.
//!
//! The first prompt of a fresh Claude spawn rides `claude`'s command line and
//! is auto-submitted once the TUI is ready. A launch can take minutes to get
//! there — a folder-trust dialog left up, a login prompt, a slow MCP startup —
//! and during that time the send is `dispatched` and the turn `AwaitingEcho`.
//! The echo-deadline sweep used to fire in that window: it requeued the send,
//! and the flush posted at the bind typed the requeued row into the pane while
//! the positional prompt was already running, so Claude Code ran the prompt
//! twice. Its `Escape` also went into whatever dialog the launch was showing.
//!
//! These tests pin the rule that replaces it: no echo deadline while the spawn
//! is unbound, and a fresh one counted from the bind.

use std::time::{Duration, Instant};

use delta_model::{SendStatus, SessionId};

use crate::interactor::session_actor::runtime::ECHO_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;
use crate::SendTarget;

const FIRST_PROMPT: &str = "first message";

/// An instant far enough past `from` that any echo wait stamped at or before
/// `from` has expired.
fn past_deadline(from: Instant) -> Instant {
    from + ECHO_DEADLINE + Duration::from_secs(1)
}

/// Start a fresh Claude session with a first prompt from the composer, and
/// return its id and the first prompt's send id. The launch is awaited, so the
/// spawn is pending (its pane is up, no hook has bound it yet).
async fn spawn_with_first_prompt(ix: &TestInteractor) -> (SessionId, i64) {
    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: TEST_WORKDIR.to_owned(),
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            FIRST_PROMPT,
            None,
        )
        .await
        .unwrap();
    assert_eq!(first.status, SendStatus::Dispatched);
    assert_eq!(
        ix.pending_session_ids().await,
        vec![first.session_id.clone()]
    );
    (first.session_id, first.id)
}

/// Nothing at all has reached the pane: no `Escape`, no typed line.
fn assert_pane_untouched(ix: &TestInteractor, context: &str) {
    assert!(
        ix.tmux_fake().pane_input.lock().unwrap().is_empty(),
        "{context}: got {:?}",
        ix.tmux_fake().pane_input.lock().unwrap()
    );
}

/// While the spawn is unbound the sweep leaves the first prompt alone, however
/// long the launch has been running.
#[tokio::test]
async fn an_unbound_spawn_keeps_its_first_prompt_past_the_echo_deadline() {
    let ix = interactor();
    let (session_id, send_id) = spawn_with_first_prompt(&ix).await;

    let dispatched = ix
        .sweep_echo_deadlines(past_deadline(Instant::now()), TICK_BOUND)
        .await;

    assert!(
        dispatched.is_empty(),
        "the sweep dispatches nothing for an unbound spawn; got {dispatched:?}"
    );
    assert_eq!(
        ix.store().send(send_id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the first prompt is still on its way on the command line, not requeued",
    );
    assert_eq!(
        ix.live_state_for(&session_id).await.turn,
        TurnState::AwaitingEcho {
            send_id,
            slash_command: false,
        },
    );
    assert_pane_untouched(
        &ix,
        "neither an Escape nor a re-type may reach a pane still launching",
    );
}

/// After a long launch, the bind restarts the wait: a sweep right after the
/// bind — in the gap before the positional prompt's `UserPromptSubmit` —
/// requeues nothing, and that submission settles the send as its echo. The
/// first prompt is never typed into the pane.
#[tokio::test]
async fn the_echo_deadline_counts_from_the_bind_after_a_slow_launch() {
    let ix = interactor();
    let (session_id, send_id) = spawn_with_first_prompt(&ix).await;

    // The launch sits on a dialog past the deadline; the sweeps meanwhile
    // leave it alone.
    ix.sweep_echo_deadlines(past_deadline(Instant::now()), TICK_BOUND)
        .await;
    // Make the wait as old as that long launch: the stamp comes from the live
    // clock, so without this the bind would follow it by milliseconds.
    ix.with_runtime(&session_id, |state| {
        state.backdate_awaiting_echo(ECHO_DEADLINE + Duration::from_secs(1))
    })
    .await;

    // The dialog is answered: `SessionStart(startup)` binds the spawn.
    let events = ix
        .on_session_start(session_start(session_id.as_str(), "startup"))
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionEvent::SessionRegistered { .. })),
        "the startup hook bound the spawn, got {events:?}"
    );
    // Let the flush the bind posted run: a no-op while the first prompt is
    // awaited.
    ix.with_runtime(&session_id, |_| ()).await;

    // The next tick lands before the positional prompt's submission.
    let dispatched = ix.sweep_echo_deadlines(Instant::now(), TICK_BOUND).await;
    assert!(
        dispatched.is_empty(),
        "the wait restarted at the bind, so nothing is re-typed; got {dispatched:?}"
    );
    assert_eq!(
        ix.store().send(send_id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );
    assert_pane_untouched(&ix, "the bound pane receives nothing before the echo");

    // The auto-submitted positional prompt is the send's echo.
    ix.transcript_fake().push(user_line("uuid-1", FIRST_PROMPT));
    let (events, _) = ix
        .on_user_prompt_submit(submit_for(
            session_id.as_str(),
            SEED_TRANSCRIPT_PATH,
            FIRST_PROMPT,
        ))
        .await
        .unwrap();
    assert!(
        events.iter().any(|event| matches!(
            event,
            SessionEvent::TurnStarted { send_id: id, .. } if *id == send_id
        )),
        "the positional prompt consumes the first send, got {events:?}"
    );
    assert!(
        !matches!(
            ix.live_state_for(&session_id).await.turn,
            TurnState::AwaitingEcho { .. }
        ),
        "the echo arrived, so nothing is awaited any more",
    );

    // Even a sweep long after: there is no outstanding send left to re-type.
    let dispatched = ix
        .sweep_echo_deadlines(past_deadline(Instant::now()), TICK_BOUND)
        .await;
    assert!(dispatched.is_empty(), "got {dispatched:?}");
    assert_pane_untouched(&ix, "the first prompt is never typed into the pane");
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "no send_line for the first prompt at any point"
    );
}

/// A positional prompt that really never echoes after the bind still meets
/// the ordinary deadline, measured from the bind: one `Escape`, then the
/// re-type.
#[tokio::test]
async fn a_bound_spawns_silent_first_prompt_is_retyped_after_the_deadline() {
    let ix = interactor();
    let (session_id, send_id) = spawn_with_first_prompt(&ix).await;

    ix.on_session_start(session_start(session_id.as_str(), "startup"))
        .await
        .unwrap();
    ix.with_runtime(&session_id, |_| ()).await;
    assert_pane_untouched(&ix, "the bind itself types nothing");

    let dispatched = ix
        .sweep_echo_deadlines(past_deadline(Instant::now()), TICK_BOUND)
        .await;
    assert!(
        dispatched.contains(&SessionEvent::SendDispatched {
            session_id: session_id.clone(),
            send_id,
        }),
        "the silent first prompt is re-dispatched; got {dispatched:?}"
    );
    let pane = ix
        .with_runtime(&session_id, |state| {
            state.handle().map(|handle| handle.pane.clone())
        })
        .await
        .expect("the spawn is bound to its pane");
    assert_eq!(
        ix.tmux_fake().pane_input.lock().unwrap().clone(),
        vec![
            PaneInput::Keys {
                pane: pane.clone(),
                keys: vec!["Escape".to_owned()],
            },
            PaneInput::Line {
                pane,
                text: FIRST_PROMPT.to_owned(),
            },
        ],
        "the retry is an Escape followed by the prompt, into the bound pane",
    );
}
