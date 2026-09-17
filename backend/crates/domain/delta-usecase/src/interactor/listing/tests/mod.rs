//! Read-side listing use-case tests.

use delta_model::SessionId;

use crate::interactor::testing::TestInteractor;
use crate::SessionPage;

mod list_sessions_page_annotates_each_with_open_state_and_threads_route_by_id;
mod list_sessions_page_annotates_open_state_and_threads;
mod list_sessions_page_breaks_recency_ties_deterministically;
mod list_sessions_page_keeps_a_stale_live_session_on_the_first_page;
mod list_sessions_page_leads_with_a_spawning_session;
mod list_sessions_page_lists_a_failed_launch_among_the_closed_ones;
mod list_sessions_page_lists_open_sessions_before_closed_ones;
mod list_sessions_page_marks_a_bound_session_open_and_a_closed_one_not;
mod list_sessions_page_reports_a_starting_pane_until_it_binds;
mod list_sessions_page_reports_no_starting_pane_for_a_failed_launch;
mod list_sessions_page_reproduces_recency_order_across_pages;
mod open_sends_for_lists_open_sends_and_rejects_unknown_session;

/// The session ids a page carries, in page order — the value the ordering
/// assertions in this module compare against.
fn listed_ids(page: &SessionPage) -> Vec<String> {
    page.listings
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect()
}

/// `(open, pane_starting)` as the session list reports them for `id` — the pair
/// the starting-pane tests in this module walk a launch through.
async fn listed_state(ix: &TestInteractor, id: &SessionId) -> (bool, bool) {
    let page = ix.list_sessions_page(None, 30).await.unwrap();
    let listing = page
        .listings
        .iter()
        .find(|listing| &listing.session.id == id)
        .expect("the session is paged");
    (listing.open, listing.pane_starting)
}
