use crate::interactor::testing::*;

/// The `open` flag tracks live state: a session with a bound pane lists as
/// `open: true`, and once closed it lists as `open: false` while still present.
/// The annotated-as-closed test above only pins the closed side, so this pins
/// the open side (and the open→closed transition) that the API surfaces.
#[tokio::test]
async fn list_sessions_marks_a_bound_session_open_and_a_closed_one_not() {
    let ix = interactor();

    // Spawn and bind a session: it now has a live pane.
    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(ix.pane_for_session(&id).await.is_some(), "bound = open");

    let open_state = |listings: &[crate::SessionListing]| {
        listings
            .iter()
            .find(|l| l.session.id == id)
            .map(|l| l.open)
            .expect("the session is listed")
    };

    assert!(
        open_state(&ix.list_sessions().await.unwrap()),
        "a bound session lists as open"
    );

    // Closing tears the pane down but keeps the row: it now lists as closed.
    ix.close_session(&id).await.unwrap();
    assert!(
        !open_state(&ix.list_sessions().await.unwrap()),
        "a closed session still lists, now as not open"
    );
}
