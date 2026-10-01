//! The session row remembers the pane a session is bound to — on a fresh
//! spawn's first hook and on a resume alike — and forgets it when the session
//! closes, so a restarted Delta re-adopts exactly the panes that are open.

use crate::interactor::testing::*;
use crate::ports::RememberedPane;

#[tokio::test]
async fn a_bound_pane_is_remembered_on_its_row_and_forgotten_on_close() {
    let ix = interactor();

    // A fresh spawn: nothing is remembered until its first hook binds it.
    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    assert_eq!(ix.store().remembered_pane(&id).await.unwrap(), None);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert_eq!(
        ix.store().remembered_pane(&id).await.unwrap(),
        Some(RememberedPane {
            tmux_session: "delta-1".into(),
            pane: "delta-1:0.0".into(),
            hooks_unreachable: false,
        }),
        "the bind writes the pane on the row"
    );

    // Closing kills the pane and clears the record.
    ix.close_session(&id).await.unwrap();
    assert_eq!(ix.store().remembered_pane(&id).await.unwrap(), None);

    // A resume binds a fresh pane, which is remembered in turn.
    ix.open_session(&id).await.unwrap();
    assert_eq!(
        ix.store().remembered_pane(&id).await.unwrap(),
        Some(RememberedPane {
            tmux_session: "delta-2".into(),
            pane: "delta-2:0.0".into(),
            hooks_unreachable: false,
        }),
        "the resume writes its new pane on the row"
    );

    ix.close_session(&id).await.unwrap();
    assert_eq!(ix.store().remembered_pane(&id).await.unwrap(), None);
}
