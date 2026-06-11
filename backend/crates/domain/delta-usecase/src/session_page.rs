//! Cursor-based paging over the session list.
//!
//! The session list is ordered most-recently-active first (see
//! [`SessionListing`](crate::SessionListing)). To page through it without a
//! limit/offset scan, a request carries an opaque cursor naming the last row of
//! the previous page; the store resumes strictly after it. The cursor's three
//! components mirror the list's three sort keys exactly, so paging reproduces
//! the single-shot order with no gap or overlap at a page boundary.

/// The position of the last row returned by a page, used to resume the next one.
///
/// The three fields are the list's sort keys, in order:
///
/// 1. `recency` — the row's last activity (`MAX(message.created_at)`), or its
///    own `created_at` when message-less. Sorted **descending**.
/// 2. `created_at` — the session's creation timestamp. Sorted **descending**.
/// 3. `id` — the session id, the final tiebreaker. Sorted **descending**:
///    Delta-minted ids are time-ordered UUID v7, so on a full timestamp tie
///    (both keys have second resolution) the newest session still sorts first.
///
/// All three are ISO-8601 UTC text (and a string session id), which compare
/// correctly as text, so no datetime casting is needed. The transport layer
/// serializes this into an opaque token; nothing outside the cursor's own
/// encode/decode helper should depend on its shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPageCursor {
    /// Last activity, or the session's `created_at` fallback (descending key).
    pub recency: String,
    /// The session's own `created_at` (descending key).
    pub created_at: String,
    /// The session id (descending tiebreaker).
    pub id: String,
}

/// One page of the session list plus the cursor to fetch the following page.
///
/// `next` is `Some` only when the page came back **full** (its length equals the
/// requested limit), signalling that more rows may follow. A short or empty page
/// is the last page and yields `None`.
#[derive(Debug)]
pub struct SessionPage {
    /// The page's listings, in most-recently-active-first order.
    pub listings: Vec<crate::SessionListing>,
    /// The cursor to resume after the last listing, or `None` on the last page.
    pub next: Option<SessionPageCursor>,
}
