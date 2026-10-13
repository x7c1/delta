//! Cursor-based paging over the session list.
//!
//! The session list is ordered open-first: every session with a live pane, then
//! every closed one, each group most-recently-active first (see
//! [`SessionListing`](crate::SessionListing)). The cursor pages the *closed*
//! stream — the live head rides on the first page alone, because liveness is
//! runtime state with no place in a stored sort key. To page through the closed
//! stream without a limit/offset scan, a request carries an opaque cursor naming
//! the last row of the previous page; the store resumes strictly after it. The
//! cursor's three components mirror the recency order's three sort keys exactly,
//! so paging reproduces the single-shot order with no gap or overlap at a page
//! boundary.

use crate::SessionPageCursor;

/// One page of the session list plus the cursor to fetch the following page.
///
/// The first page carries every live session (open, or a spawn in flight) plus
/// up to `limit` closed ones; later pages carry closed sessions only. `limit`
/// therefore bounds the *closed* portion, not the whole page.
///
/// `next` is `Some` only when that closed portion came back **full** (`limit`
/// closed rows were kept), signalling that more rows may follow. A short or
/// empty closed portion is the last page and yields `None`.
#[derive(Debug)]
pub struct SessionPage {
    /// The page's listings: the live ones first (first page only), then the
    /// closed ones, each group most-recently-active first.
    pub listings: Vec<crate::SessionListing>,
    /// The cursor to resume after the last listing, or `None` on the last page.
    pub next: Option<SessionPageCursor>,
}
