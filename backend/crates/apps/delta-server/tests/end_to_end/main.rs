//! Application-level integration test.
//!
//! This is the black-box test binary for the whole backend. Each test
//! assembles the real router with test-wired gateways — an in-memory
//! `SqliteStore`, a `JsonlTranscript` reading a temp file we control, and a
//! no-op `TmuxDriver` so the tests run in CI without a real tmux — and drives a
//! realistic flow across the REST and hook surface through `router()` using
//! `tower::ServiceExt::oneshot` (no network bind).
//!
//! The flow exercised end to end (steps 1–6 in [`send_and_turn_correlation`],
//! step 7 in [`session_removal`]):
//!
//! 1. The first `UserPromptSubmit` hook registers the session.
//! 2. `GET /api/sessions` lists it, annotated with its open state and `main`
//!    thread id.
//! 3. `POST /api/sends` enqueues a send into that thread (with a locator quote),
//!    which routes to the thread's owning session.
//! 4. The matching `UserPromptSubmit` hook correlates against the FIFO head:
//!    the send is marked matched, the locator quote is returned inside the
//!    `hookSpecificOutput.additionalContext` envelope, and the transcript line
//!    is ingested.
//! 5. `GET /api/sessions/{id}/threads` and `GET /api/threads/{id}/messages`
//!    reflect the resulting thread and message state.
//! 6. `POST /api/sessions/{id}/close` tears the pane down; the session then
//!    lists as closed, and open/close/threads on an unknown id are `404`.
//! 7. `DELETE /api/sessions/{id}` removes a closed session outright — its rows
//!    go (a session outside a Delta worktree leaves nothing on disk to
//!    remove) — and is refused with a stable `code` while the session is open
//!    or still starting.
//!
//! Beyond that flow, the tests cover new-session sends, the create-session
//! endpoint, the async event seam, and what the session list carries. They are
//! grouped by subject, one module each; [`support`] holds the fake gateways,
//! the app assembly, and the request helpers they share.

mod support;

mod async_events;
mod new_session_sends;
mod send_and_turn_correlation;
mod session_creation;
mod session_list;
mod session_removal;
