//! When the server could not get its previous port back, a surviving agent's
//! hooks still go to the old port, where another process may now listen. So a
//! pane found alive is not kept: boot ends it through the ordinary close and
//! records why, and the next send resumes the session with hooks aimed at the
//! new port. A start on the recorded port keeps such a pane running.

use crate::interactor::lifecycle::adopt_pane::PORT_MOVED_REASON;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::HookEndpointChange;

use super::readoption_support::{left_behind, listing, survives};

#[tokio::test]
async fn boot_ends_a_surviving_session_when_the_port_moved() {
    let (ix, mut sink) = interactor_with_event_sink();
    let alive = left_behind(&ix, "sess-alive", "delta-7").await;
    let gone = left_behind(&ix, "sess-gone", "delta-8").await;
    survives(&ix, "delta-7");

    let summary = ix
        .readopt_surviving_sessions(HookEndpointChange::PortMoved)
        .await
        .unwrap();

    assert_eq!(summary.ended, 1);
    assert_eq!(summary.adopted, 0);
    assert_eq!(summary.gone, 1);

    // The surviving pane was killed and the session is closed, with nothing
    // left remembered for a later restart to adopt.
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-7".to_owned()]
    );
    let row = listing(&ix, &alive).await;
    assert!(!row.open);
    assert!(!row.hooks_unreachable);
    assert_eq!(ix.store().remembered_pane(&alive).await.unwrap(), None);

    // The reason is recorded for the closed session's screen, and the browser
    // hears that the session closed.
    let session = ix.store().session(&alive).await.unwrap().unwrap();
    assert_eq!(session.closed_reason.as_deref(), Some(PORT_MOVED_REASON));
    let mut closed = Vec::new();
    while let Ok(event) = sink.try_recv() {
        if let SessionEvent::SessionClosed { session_id } = event {
            closed.push(session_id);
        }
    }
    assert_eq!(closed, vec![alive.clone()]);

    // The session whose pane was already gone gets no reason: Delta did not
    // close it.
    let session = ix.store().session(&gone).await.unwrap().unwrap();
    assert_eq!(session.closed_reason, None);

    // Opening it again resumes it with a fresh launch rather than adopting
    // anything.
    ix.open_session(&alive).await.unwrap();
    ix.await_launch().await;
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "the session is resumed in a new pane");
    assert!(
        created[0].command.iter().any(|arg| arg == "--resume"),
        "{:?}",
        created[0].command
    );
}

/// A start on the recorded port changes nothing: the surviving pane keeps
/// running and its session is open on it again.
#[tokio::test]
async fn boot_on_the_recorded_port_keeps_a_surviving_session_running() {
    let ix = interactor();
    let alive = left_behind(&ix, "sess-alive", "delta-7").await;
    survives(&ix, "delta-7");

    let summary = ix
        .readopt_surviving_sessions(HookEndpointChange::Unchanged)
        .await
        .unwrap();

    assert_eq!(summary.adopted, 1);
    assert_eq!(summary.ended, 0);
    assert!(ix.tmux_fake().killed.lock().unwrap().is_empty());
    assert!(listing(&ix, &alive).await.open);
    let session = ix.store().session(&alive).await.unwrap().unwrap();
    assert_eq!(session.closed_reason, None);
}

/// Binding a pane again clears the reason a boot recorded: the session is open,
/// so the reason no longer describes it.
#[tokio::test]
async fn binding_a_pane_again_clears_the_closed_reason() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", "delta-7").await;
    ix.store()
        .record_closed_reason(&id, PORT_MOVED_REASON)
        .await
        .unwrap();

    ix.store()
        .remember_pane(
            &id,
            &crate::ports::RememberedPane {
                tmux_session: "delta-9".into(),
                pane: super::readoption_support::pane_for("delta-9"),
                hooks_unreachable: false,
            },
        )
        .await
        .unwrap();

    let session = ix.store().session(&id).await.unwrap().unwrap();
    assert_eq!(session.closed_reason, None);
}
