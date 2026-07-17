//! The wire form of the provider-availability listing (`GET /api/providers`).

use delta_model::ProviderAvailability;
use serde::Serialize;
use ts_rs::TS;

use crate::session::WireAgentProvider;

/// JSON shape of one provider's launch availability.
///
/// Mirrors the domain [`ProviderAvailability`] field-for-field. `available`
/// reports whether the provider's launch binary is present on the server host
/// (v1 checks binary presence only); `detail` carries a human-readable reason
/// when it is not, which the new-session selector shows next to the disabled
/// option. The `detail`-carrying shape leaves room for a future
/// version-compatibility verdict without a breaking reshape (see the domain
/// type's docs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "ProviderAvailability")]
pub struct WireProviderAvailability {
    pub provider: WireAgentProvider,
    pub available: bool,
    /// A reason string when `available` is `false`; `null` when available.
    pub detail: Option<String>,
}

impl From<ProviderAvailability> for WireProviderAvailability {
    fn from(availability: ProviderAvailability) -> Self {
        WireProviderAvailability {
            provider: availability.provider.into(),
            available: availability.available,
            detail: availability.detail,
        }
    }
}

/// JSON shape of `GET /api/providers`: launch availability for every known
/// provider.
///
/// Wrapped in an object (rather than a bare array) so the response can grow
/// sibling fields later without breaking the contract, mirroring
/// [`WirePullRequestsResponse`](crate::rest::WirePullRequestsResponse).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "ProvidersResponse")]
pub struct WireProvidersResponse {
    pub providers: Vec<WireProviderAvailability>,
}

impl From<Vec<ProviderAvailability>> for WireProvidersResponse {
    fn from(list: Vec<ProviderAvailability>) -> Self {
        WireProvidersResponse {
            providers: list.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::AgentProvider;

    #[test]
    fn available_provider_serializes_with_null_detail() {
        let response = WireProvidersResponse::from(vec![ProviderAvailability {
            provider: AgentProvider::Claude,
            available: true,
            detail: None,
        }]);
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({
                "providers": [
                    { "provider": "claude", "available": true, "detail": null }
                ]
            }),
        );
    }

    #[test]
    fn unavailable_provider_carries_its_reason() {
        let response = WireProvidersResponse::from(vec![ProviderAvailability {
            provider: AgentProvider::Codex,
            available: false,
            detail: Some("The 'codex' binary for codex was not found on PATH.".to_owned()),
        }]);
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["providers"][0]["provider"], "codex");
        assert_eq!(value["providers"][0]["available"], false);
        assert_eq!(
            value["providers"][0]["detail"],
            "The 'codex' binary for codex was not found on PATH."
        );
    }
}
