//! `POST /api/auth/exchange` and `POST /api/auth/codes` — the one-time code
//! handoff of the per-run bearer token (see `crate::one_time_codes`).

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use delta_wire::rest::{WireAuthCodeResponse, WireAuthExchangeRequest, WireAuthExchangeResponse};

use crate::state::AppState;

/// Neither response may be kept by a cache: both carry a credential.
const NO_STORE: [(header::HeaderName, &str); 1] = [(header::CACHE_CONTROL, "no-store")];

/// `POST /api/auth/exchange` — trade a one-time code for the bearer token.
///
/// Reached without a bearer token (the caller has none yet; see
/// `crate::auth_guard`). A code that was minted here, is unused and has not
/// expired is spent and answered with the token; any other answers `401` with
/// no body. A body without a `code` is refused by the extractor (`4xx`).
pub(crate) async fn exchange_auth_code(
    State(state): State<AppState>,
    Json(request): Json<WireAuthExchangeRequest>,
) -> Response {
    if !state.redeem_one_time_code(&request.code) {
        tracing::warn!("refused a one-time code that is used, expired or unknown");
        return StatusCode::UNAUTHORIZED.into_response();
    }
    (
        NO_STORE,
        Json(WireAuthExchangeResponse {
            token: state.token().to_owned(),
        }),
    )
        .into_response()
}

/// `POST /api/auth/codes` — mint a one-time code, for `delta-server open-url`.
/// Behind the bearer guard like the rest of `/api/*`.
pub(crate) async fn mint_auth_code(State(state): State<AppState>) -> Response {
    (
        NO_STORE,
        Json(WireAuthCodeResponse {
            code: state.mint_one_time_code(),
        }),
    )
        .into_response()
}
