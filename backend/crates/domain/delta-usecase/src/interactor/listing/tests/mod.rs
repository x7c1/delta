//! Read-side listing use-case tests.

use crate::SessionPage;

mod list_sessions_page_annotates_each_with_open_state_and_threads_route_by_id;
mod list_sessions_page_annotates_open_state_and_threads;
mod list_sessions_page_breaks_recency_ties_deterministically;
mod list_sessions_page_keeps_a_stale_live_session_on_the_first_page;
mod list_sessions_page_leads_with_a_spawning_session;
mod list_sessions_page_lists_open_sessions_before_closed_ones;
mod list_sessions_page_marks_a_bound_session_open_and_a_closed_one_not;
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
