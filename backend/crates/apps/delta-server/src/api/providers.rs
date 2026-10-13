//! `GET /api/providers` — what each agent provider can do and whether it can
//! launch.

use axum::extract::State;
use axum::Json;

use delta_wire::rest::WireProvidersResponse;

use crate::state::AppState;

/// `GET /api/providers` — launch availability and capability profile for every
/// known agent provider.
///
/// For each provider (Claude, Codex) reports whether its configured launch
/// binary is present on the server host, with a reason string when it is not,
/// plus the provider's UI-relevant capability profile (e.g. whether it offers an
/// attachable terminal). The new-session provider selector disables an
/// unavailable provider and shows the reason, so a user cannot pick a provider
/// that would fail at spawn; the workspace reads the capability profile to gate
/// provider-specific surfaces (the terminal tab is hidden for a provider with no
/// terminal). Always `200`: a missing binary is data (`available: false`), never
/// an error.
pub(crate) async fn list_providers(State(state): State<AppState>) -> Json<WireProvidersResponse> {
    let availability = state.interactor().provider_availability().await;
    // Pair each provider's runtime launch availability with its static
    // capability profile. The profile comes from the composition root's
    // per-provider accessor (which reads the same const each adapter's
    // `capabilities()` returns), so it is resolved without a live adapter and
    // can never drift from what a running adapter reports.
    let entries = availability
        .into_iter()
        .map(|availability| {
            let capabilities = delta_bootstrap::provider_capabilities(availability.provider);
            (availability, capabilities)
        })
        .collect::<Vec<_>>();
    Json(WireProvidersResponse::from(entries))
}
