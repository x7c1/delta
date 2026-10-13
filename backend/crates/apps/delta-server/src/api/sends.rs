//! `/api/sends` — enqueueing, cancelling and releasing sends.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use delta_wire::rest::{WireCreateSendRequest, WireSendResponse};

use crate::state::AppState;

use super::ApiError;

/// `POST /api/sends` — enqueue a send into a session named by the request.
///
/// The session is derived from the target thread for an existing send, or
/// created for a `new_session` send. Turn confirmation arrives later via the
/// `UserPromptSubmit` hook; only enqueue-time events (e.g. `send_dispatched`
/// from the idle-flush) are broadcast here.
pub(crate) async fn create_send(
    State(state): State<AppState>,
    Json(req): Json<WireCreateSendRequest>,
) -> Result<(StatusCode, Json<WireSendResponse>), ApiError> {
    let (target, text, locator_quote) = req
        .into_target()
        .map_err(|err| ApiError::BadRequest(err.message().to_owned()))?;
    let (send, events) = state
        .interactor()
        .enqueue_send(target, &text, locator_quote.as_deref())
        .await?;
    // The enqueue may have promoted a previously-queued send (the idle-flush
    // safety net); broadcast so the browser sees the queued->dispatched
    // transition immediately.
    state.broadcast(events);
    Ok((StatusCode::CREATED, Json(WireSendResponse::from(send))))
}

/// `POST /api/sends/{id}/cancel` — cancel a queued or dispatched send (204).
///
/// A send composed while the assistant's turn is in flight is held in the
/// `queued` state until the session goes idle; this abandons such a send
/// before that dispatch. A `dispatched` send the turn machine is awaiting
/// (its echo has not arrived — typically the user pressed `Escape` in the
/// TUI to discard the composer buffer, leaving no signal Delta can observe)
/// is cancelled by injecting a single `Escape` keystroke into the pane and
/// dropping the row to `cancelled` — any send queued behind the cancelled
/// head then promotes through the existing idle-flush. A `dispatched` row
/// the turn machine holds no claim on is cancelled as a pure state
/// transition — no keystroke is injected and the turn machine is untouched.
/// The row flips to `cancelled` in every success case and drops out of the
/// open-send list (the browser refetches that list to clear the chip — no
/// event is broadcast).
///
/// Replies `409` with code `send_not_cancellable` when the send no longer
/// exists, is already terminal (matched a transcript line, or already
/// cancelled), or is `dispatched` but its echo has already arrived (the
/// turn carries it in flight; the user reaches for the in-flight interrupt
/// instead). The browser drops its cancel control and reconciles from the
/// refetch on this code.
pub(crate) async fn cancel_send(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.interactor().cancel_send(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sends/{id}/release` — release a *held* send into the normal
/// queued flow (204).
///
/// Two paths leave a row `queued` with a `held_at` marker — visible in the
/// open-send list, but never auto-dispatched: the boot-time reconcile,
/// recovering every send a dead server process left `dispatched`, and the
/// echo-deadline park, for a send whose keystrokes were swallowed without a
/// trace twice running.
///
/// This endpoint is the explicit "Send" action on such a row: it first ensures
/// the owning session is open (resuming `claude --resume <id>` when it is
/// closed, the normal state right after a restart), then clears the marker (a
/// guarded UPDATE, so a race with a cancel is a clean conflict) and runs the
/// session's normal queued dispatch — if the session was already open and idle
/// the row types immediately (the `send_dispatched` event is broadcast); if the
/// release resumed the session the row is typed by the resume-settle flush once
/// the resumed pane accepts input; otherwise (mid-turn) it waits as an ordinary
/// queued send for the turn-end trigger. The sibling Cancel action is the
/// existing [`cancel_send`] — a held row's status is still `queued`, so the
/// guarded queued cancel already covers it.
///
/// Replies `409` with code `send_not_releasable` when the send is unknown,
/// was never held, is already released, or has since been cancelled.
/// The browser drops its Send control and reconciles from the refetch on
/// this code. An ensure-open failure surfaces on its own path — e.g. `409`
/// `resume_unavailable` when the session's transcript is gone — before the
/// marker is touched, so the release can be retried.
pub(crate) async fn release_send(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let events = state.interactor().release_send(id).await?;
    // The release may have dispatched the released (or an older queued) send;
    // broadcast so the browser sees the queued→dispatched transition.
    state.broadcast(events);
    Ok(StatusCode::NO_CONTENT)
}
