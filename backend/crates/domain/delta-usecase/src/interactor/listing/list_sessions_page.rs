use crate::error::Result;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::session_listing::SessionListing;
use crate::session_page::{SessionPage, SessionPageCursor};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
{
    /// One page of the session list, ordered most-recently-active first, with
    /// an opaque-able cursor to fetch the next page.
    ///
    /// This is the paginated form of [`Self::list_sessions`]: the store pushes
    /// the recency ordering into SQL and returns at most `limit` rows plus each
    /// row's inline `last_activity_at`, so there is no per-row activity lookup.
    /// Each row is then enriched with its live `open` state (process-runtime
    /// data the registry owns, not a SQL column) and its `main` thread id.
    ///
    /// The returned [`SessionPage::next`] cursor names the last listing's
    /// `(recency, created_at, id)` so the caller can resume strictly after it;
    /// it is `Some` only when the page came back full (more rows may follow).
    pub async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<SessionPage> {
        let rows = self.store.list_sessions_page(cursor, limit).await?;
        let full = rows.len() as u32 == limit;

        let mut listings = Vec::with_capacity(rows.len());
        for (session, last_activity_at) in rows {
            let main_thread_id = self.store.main_thread_id(&session.id).await?;
            let open = self.is_session_open(&session.id).await;
            listings.push(SessionListing {
                session,
                open,
                main_thread_id,
                last_activity_at,
            });
        }

        // The next cursor names the last row's sort key, where `recency` is the
        // listing's `last_activity_at` or its `created_at` fallback. It is only
        // meaningful when the page was full; a short/last page yields `None`.
        let next = match (full, listings.last()) {
            (true, Some(last)) => Some(SessionPageCursor {
                recency: last
                    .last_activity_at
                    .clone()
                    .unwrap_or_else(|| last.session.created_at.clone()),
                created_at: last.session.created_at.clone(),
                id: last.session.id.as_str().to_owned(),
            }),
            _ => None,
        };

        Ok(SessionPage { listings, next })
    }
}
