use crate::error::Result;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::session_listing::SessionListing;
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Every registered session, annotated with its live state and `main` thread.
    ///
    /// Lists all sessions from the store (ordered by creation) and tags each with
    /// whether the registry currently holds a live pane for it, plus its trunk
    /// thread id. This is the browser's hydration surface: it shows every known
    /// conversation — open or closed — so the navigator can route into any of
    /// them. Returns an empty list until the first `UserPromptSubmit` registers a
    /// session (Claude Code never fires `SessionStart`).
    pub async fn list_sessions(&self) -> Result<Vec<SessionListing>> {
        let sessions = self.store.list_sessions().await?;
        let mut out = Vec::with_capacity(sessions.len());
        for session in sessions {
            let main_thread_id = self.store.main_thread_id(&session.id).await?;
            let open = self.is_session_open(&session.id).await;
            let last_activity_at = self.store.last_activity_at(&session.id).await?;
            out.push(SessionListing {
                session,
                open,
                main_thread_id,
                last_activity_at,
            });
        }
        // Most-recently-active first. The recency key is the session's last
        // activity (`MAX(message.created_at)`), falling back to its own
        // `created_at` when it has no messages yet — a brand-new, message-less
        // session sorts near the top because its `created_at` is "now". Ties
        // break deterministically on `created_at` then `id` so equal-activity
        // sessions keep a stable order across calls. ISO-8601 UTC timestamps
        // are lexicographically ordered, so a string compare is a time compare.
        out.sort_by(|a, b| {
            // Recency key: last activity, or the session's own `created_at`
            // when message-less.
            fn recency(s: &SessionListing) -> &str {
                s.last_activity_at
                    .as_deref()
                    .unwrap_or(s.session.created_at.as_str())
            }
            // Reverse all three keys so the most recent comes first; the `id`
            // tiebreaker is descending too because Delta-minted ids are
            // time-ordered UUID v7 — on a full timestamp tie (both keys have
            // second resolution) the newest session still sorts first.
            recency(b)
                .cmp(recency(a))
                .then_with(|| b.session.created_at.cmp(&a.session.created_at))
                .then_with(|| b.session.id.as_str().cmp(a.session.id.as_str()))
        });
        Ok(out)
    }
}
