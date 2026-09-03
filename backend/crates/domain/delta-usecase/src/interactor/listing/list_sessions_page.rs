use std::collections::HashSet;

use delta_model::SessionId;

use crate::error::Result;
use crate::ports::{GitWorktree, SessionPageRow, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::session_listing::SessionListing;
use crate::session_page::{SessionPage, SessionPageCursor};
use crate::Interactor;

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// One page of the session list, ordered **open-first**, with an
    /// opaque-able cursor to fetch the next page.
    ///
    /// Every session that currently has a live pane leads the list, then every
    /// closed one; inside each group the order is most-recently-active first
    /// (`recency` DESC, `created_at` DESC, `id` DESC, where `recency` is
    /// `last_activity_at` falling back to `created_at`). A closed session is
    /// never the thing the user is about to act on, so it never outranks a live
    /// one however recently its transcript was touched. "Live" is wider than
    /// `open`: a spawn still in flight counts (it pages as `open: false` until
    /// its first hook binds it), so a just-started session leads the list from
    /// the moment it is accepted rather than jumping up seconds later.
    ///
    /// Liveness is process-runtime state the registry owns, not a SQL column,
    /// so the ordering is assembled here rather than pushed into the query —
    /// which also keeps the store's recency `ORDER BY` served by its expression
    /// index. Two phases:
    ///
    /// 1. On the first page only (`cursor: None`), the live sessions' rows are
    ///    fetched by id ([`SessionStore::list_sessions_by_ids`]) and emitted
    ///    first. The set is bounded by the number of live panes.
    /// 2. Every page then draws from the store's recency stream, over-fetching
    ///    by the live count and dropping the live rows, so at least `limit`
    ///    closed rows come back whenever that many remain. The `next` cursor
    ///    names the last **kept** row and is `Some` only when `limit` closed
    ///    rows were kept — a short page is the end of the stream.
    ///
    /// So the first page carries every live session plus up to `limit` closed
    /// ones, and later pages carry closed sessions only.
    ///
    /// Liveness is snapshotted per call, so a session whose state flips
    /// mid-walk can be listed twice or not at all. Closing between two fetches
    /// duplicates it: it led page one as live and, now closed, is no longer
    /// filtered out of a later page's stream. Opening between two fetches drops
    /// it: it is filtered out of the later page's stream while the live head
    /// that would carry it rode on page one. Neither is corrected here — the
    /// browser invalidates the whole list on `session_opened` /
    /// `session_closed`, which is exactly when either can happen, so the walk
    /// restarts against a fresh snapshot immediately.
    pub async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<SessionPage> {
        let live_ids = self.live_session_ids().await;
        let live: HashSet<SessionId> = live_ids.iter().cloned().collect();

        let mut listings = Vec::new();

        // Phase 1 — the live head, first page only. Later pages resume inside
        // the closed stream, which these rows are excluded from, so emitting
        // them again would duplicate them.
        if cursor.is_none() {
            for row in self.store.list_sessions_by_ids(&live_ids).await? {
                listings.push(self.listing_for(row).await?);
            }
        }

        // Phase 2 — the closed stream. Over-fetch by exactly the live count so
        // that dropping the live rows still leaves `limit` closed ones whenever
        // that many remain; a page is short only when the stream is exhausted.
        let rows = self
            .store
            .list_sessions_page(cursor, limit.saturating_add(live.len() as u32))
            .await?;
        let mut kept = 0;
        let mut last_kept = None;
        for row in rows {
            if kept == limit {
                break;
            }
            if live.contains(&row.0.id) {
                continue;
            }
            let listing = self.listing_for(row).await?;
            last_kept = Some(SessionPageCursor {
                recency: listing
                    .last_activity_at
                    .clone()
                    .unwrap_or_else(|| listing.session.created_at.clone()),
                created_at: listing.session.created_at.clone(),
                id: listing.session.id.as_str().to_owned(),
            });
            listings.push(listing);
            kept += 1;
        }

        // The next cursor names the last kept row's sort key in the closed
        // stream — never the last row *fetched*, which may be a dropped live one
        // or an over-fetched extra. It is meaningful only when the closed
        // portion came back full; a short/last page yields `None`.
        let next = if kept == limit { last_kept } else { None };

        Ok(SessionPage { listings, next })
    }

    /// Enrich one store row into a [`SessionListing`]: attach its trunk thread
    /// and its live `open` state (process-runtime data the registry owns, not a
    /// SQL column).
    async fn listing_for(&self, row: SessionPageRow) -> Result<SessionListing> {
        let (session, last_activity_at) = row;
        let main_thread_id = self.store.main_thread_id(&session.id).await?;
        let open = self.is_session_open(&session.id).await;
        Ok(SessionListing {
            session,
            open,
            main_thread_id,
            last_activity_at,
        })
    }
}
