//! `GET /api/version` — the Delta workspace version.

use axum::Json;

use delta_wire::rest::WireVersionResponse;

/// `GET /api/version` — the Delta workspace version, pre-formatted for the
/// browser footer.
///
/// Owned by the server (not the browser) so the format contract lives in one
/// place: `release` builds (by build origin) return `v<version>`, `local`
/// builds return `v<version>+dev.<short-sha>` whatever the profile. See
/// `crate::version` for the rationale on `+dev` (SemVer build metadata) vs
/// `-dev` (pre-release).
pub(crate) async fn get_version() -> Json<WireVersionResponse> {
    Json(WireVersionResponse {
        version: crate::version::display_version(),
    })
}
