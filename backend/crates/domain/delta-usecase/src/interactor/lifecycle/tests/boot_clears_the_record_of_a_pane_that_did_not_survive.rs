//! At boot, a session whose remembered tmux session no longer exists stays
//! closed, and its record is cleared so no later step goes looking for it.

use crate::interactor::testing::*;

use super::readoption_support::{left_behind, SURVIVING_TOKEN};

#[tokio::test]
async fn boot_clears_the_record_of_a_pane_that_did_not_survive() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    // `SURVIVING_TOKEN` is not running: the pane ended while Delta was down.

    let summary = ix.readopt_surviving_sessions(false).await.unwrap();

    assert_eq!(summary.gone, 1);
    assert_eq!(summary.adopted, 0);
    assert!(!ix.is_session_open(&id).await, "the session stays closed");
    assert_eq!(ix.bound_pane(&id).await, None);
    assert_eq!(
        ix.store().remembered_pane(&id).await.unwrap(),
        None,
        "the remembered pane is cleared"
    );
    assert!(ix.tmux_fake().created.lock().unwrap().is_empty());
}
