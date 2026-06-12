use delta_model::SessionId;
use tokio::sync::oneshot;

use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::testing::*;
use crate::ports::StopHook;
use crate::turn::TurnState;

/// The mailbox makes interleaving scriptable: a whole sequence of inputs is
/// posted to one actor *before any of them executes*, and each query observes
/// exactly the state between its neighbours — an external prompt flips the
/// turn in flight, the stop returns it to idle, and the two interleaved
/// queries see those two states, never a torn or reordered one. Under the
/// lock-era design this ordering depended on which caller won which mutex.
#[tokio::test]
async fn a_scripted_input_sequence_executes_in_mailbox_order() {
    let ix = interactor();
    ix.seed_session().await;
    let id = SessionId::from("sess-1");

    // Script: external prompt → query → stop → query, posted back-to-back
    // with no awaits in between (nothing has executed yet when the last one
    // is queued).
    let (prompt_tx, prompt_rx) = oneshot::channel();
    ix.sessions.post(
        &id,
        SessionInput::UserPromptSubmit {
            hook: submit("typed straight into the pane"),
            reply: prompt_tx,
        },
    );
    let (mid_tx, mid_rx) = oneshot::channel();
    ix.sessions.post(&id, SessionInput::QueryTurnState { reply: mid_tx });
    let (stop_tx, stop_rx) = oneshot::channel();
    ix.sessions.post(
        &id,
        SessionInput::Stop {
            hook: StopHook {
                session_id: id.clone(),
                stop_reason: None,
            },
            reply: stop_tx,
        },
    );
    let (end_tx, end_rx) = oneshot::channel();
    ix.sessions.post(&id, SessionInput::QueryTurnState { reply: end_tx });

    let (_events, _context) = prompt_rx.await.unwrap().unwrap();
    assert_eq!(
        mid_rx.await.unwrap(),
        TurnState::InFlight { send_id: None },
        "the first query runs strictly after the prompt, before the stop"
    );
    stop_rx.await.unwrap().unwrap();
    assert_eq!(
        end_rx.await.unwrap(),
        TurnState::Idle,
        "the second query runs strictly after the stop"
    );
}
