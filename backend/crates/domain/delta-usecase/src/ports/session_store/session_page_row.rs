//! One row of a session-list page.

use delta_model::Session;

/// One row of a session-list page: the stored session plus its `last_activity_at`
/// (the denormalized `MAX(message.created_at)`, `None` when message-less), read
/// from the session row by the page query so the usecase needs no per-row
/// follow-up lookup.
pub type SessionPageRow = (Session, Option<String>);
