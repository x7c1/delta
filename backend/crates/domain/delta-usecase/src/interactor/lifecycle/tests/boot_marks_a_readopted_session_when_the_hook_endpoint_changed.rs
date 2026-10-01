//! When the hook endpoint changed since the previous run, a re-adopted
//! session's agent still calls the old URLs, so it is marked as unable to
//! deliver hooks — and only the sessions actually re-adopted are: the flag is
//! also set on a first run, when there is nothing to strand.

use crate::interactor::testing::*;

use super::readoption_support::{left_behind, listing, survives};

#[tokio::test]
async fn boot_marks_a_readopted_session_when_the_hook_endpoint_changed() {
    let ix = interactor();
    let alive = left_behind(&ix, "sess-alive", "delta-7").await;
    let gone = left_behind(&ix, "sess-gone", "delta-8").await;
    survives(&ix, "delta-7");

    let summary = ix.readopt_surviving_sessions(true).await.unwrap();

    assert_eq!(summary.adopted, 1);
    assert_eq!(summary.hooks_unreachable, 1);
    assert_eq!(summary.gone, 1);

    // The re-adopted session carries the mark on its list row…
    let row = listing(&ix, &alive).await;
    assert!(row.open);
    assert!(row.hooks_unreachable);
    // …while the session whose pane was gone is just closed, with no mark.
    let row = listing(&ix, &gone).await;
    assert!(!row.open);
    assert!(!row.hooks_unreachable);

    // The mark is persisted with the pane, so a later restart whose endpoint
    // happens to match this run's does not forget it.
    assert!(
        ix.store()
            .remembered_pane(&alive)
            .await
            .unwrap()
            .expect("still remembered")
            .hooks_unreachable
    );
    assert_eq!(ix.store().remembered_pane(&gone).await.unwrap(), None);

    // Closing the session (what the notice tells the user to do) clears the
    // mark along with the pane.
    ix.close_session(&alive).await.unwrap();
    assert!(!listing(&ix, &alive).await.hooks_unreachable);
    assert_eq!(ix.store().remembered_pane(&alive).await.unwrap(), None);
}

/// An unchanged endpoint marks nothing: the surviving agent's hooks still reach
/// this server.
#[tokio::test]
async fn boot_with_an_unchanged_endpoint_leaves_a_readopted_session_unmarked() {
    let ix = interactor();
    let alive = left_behind(&ix, "sess-alive", "delta-7").await;
    survives(&ix, "delta-7");

    ix.readopt_surviving_sessions(false).await.unwrap();

    let row = listing(&ix, &alive).await;
    assert!(row.open);
    assert!(!row.hooks_unreachable);
}

/// A pane tmux could not be asked about at boot may still be running with the
/// old hook URLs, so a changed endpoint is recorded on it then: the send that
/// later adopts it through the resume backstop cannot tell a changed endpoint
/// from an unchanged one, and must still show the notice.
#[tokio::test]
async fn an_unprobed_pane_keeps_the_mark_for_the_send_that_adopts_it_later() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-alive", "delta-7").await;
    survives(&ix, "delta-7");
    ix.tmux_fake().fail_probes();

    let summary = ix.readopt_surviving_sessions(true).await.unwrap();

    assert_eq!(summary.unprobed, 1);
    assert_eq!(summary.adopted, 0);
    assert!(!ix.is_session_open(&id).await);
    assert!(
        ix.store()
            .remembered_pane(&id)
            .await
            .unwrap()
            .expect("the record is kept")
            .hooks_unreachable
    );

    // tmux answers again; the next open adopts the pane with the mark.
    *ix.tmux_fake().probe_fails.lock().unwrap() = false;
    ix.open_session(&id).await.unwrap();

    assert!(ix.tmux_fake().created.lock().unwrap().is_empty());
    let row = listing(&ix, &id).await;
    assert!(row.open);
    assert!(row.hooks_unreachable);
}
