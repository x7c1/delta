use crate::interactor::testing::*;

/// The `open` flag tracks live state: a session with a bound pane pages as
/// `open: true`, and once closed it pages as `open: false` while still present.
/// `list_sessions_page_annotates_open_state_and_threads` only pins the open
/// side, so this pins the open→closed transition the API surfaces.
#[tokio::test]
async fn list_sessions_page_marks_a_bound_session_open_and_a_closed_one_not() {
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

    let open_state = |page: &crate::session_page::SessionPage| {
        page.listings
            .iter()
            .find(|l| l.session.id == id)
            .map(|l| l.open)
            .expect("the session is paged")
    };

    assert!(
        open_state(&ix.list_sessions_page(None, 30).await.unwrap()),
        "a bound session pages as open"
    );

    // Closing tears the pane down but keeps the row: it now pages as closed.
    ix.close_session(&id).await.unwrap();
    assert!(
        !open_state(&ix.list_sessions_page(None, 30).await.unwrap()),
        "a closed session still pages, now as not open"
    );
}
