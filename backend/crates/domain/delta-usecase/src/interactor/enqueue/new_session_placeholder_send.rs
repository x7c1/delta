use delta_model::{Send, SessionId, ThreadId};

/// Build the synthetic, not-yet-persisted [`Send`] returned for a
/// composer-first send that spawned a fresh session.
///
/// No `send` row exists yet — it would reference a session id that does not
/// exist until the first `UserPromptSubmit` binds the spawn. This shapes a
/// response for the REST surface meanwhile: `id` is `0` (no row), the status is
/// `Dispatched`, and both the session id and target thread are left empty/`0`
/// because neither exists yet (the real row is written on the new session's
/// `main` thread at bind time).
pub(in crate::interactor::enqueue) fn new_session_placeholder_send(
    text: &str,
    locator_quote: Option<&str>,
) -> Send {
    Send {
        id: 0,
        session_id: SessionId::from(""),
        thread_id: ThreadId(0),
        semantic_parent_uuid: None,
        text: text.to_owned(),
        locator_quote: locator_quote.map(str::to_owned),
        status: delta_model::SendStatus::Dispatched,
        matched_uuid: None,
        created_at: String::new(),
    }
}
