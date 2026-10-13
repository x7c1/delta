//! JSON shape of one provider's launch availability plus its capability profile.

use delta_model::ProviderAvailability;
use delta_usecase::AgentCapabilities;
use serde::Serialize;
use ts_rs::TS;

use crate::agent_provider::WireAgentProvider;
use crate::rest::provider_capabilities::WireProviderCapabilities;

/// JSON shape of one provider's launch availability plus its capability profile.
///
/// `provider`/`available`/`detail` mirror the domain [`ProviderAvailability`]:
/// `available` reports whether the provider's launch binary is present on the
/// server host (v1 checks binary presence only); `detail` carries a
/// human-readable reason when it is not, which the new-session selector shows
/// next to the disabled option. The `detail`-carrying shape leaves room for a
/// future version-compatibility verdict without a breaking reshape (see the
/// domain type's docs). `capabilities` carries the UI-relevant capability
/// profile — resolved statically per provider, so it is present even for an
/// unavailable provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "ProviderAvailability")]
pub struct WireProviderAvailability {
    pub provider: WireAgentProvider,
    pub available: bool,
    /// A reason string when `available` is `false`; `null` when available.
    pub detail: Option<String>,
    pub capabilities: WireProviderCapabilities,
}

impl From<(ProviderAvailability, AgentCapabilities)> for WireProviderAvailability {
    fn from((availability, capabilities): (ProviderAvailability, AgentCapabilities)) -> Self {
        WireProviderAvailability {
            provider: availability.provider.into(),
            available: availability.available,
            detail: availability.detail,
            capabilities: capabilities.into(),
        }
    }
}
