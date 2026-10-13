//! JSON shape of a session's AI-agent provider.

use delta_model::AgentProvider;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// JSON shape of a session's AI-agent provider.
///
/// Mirrors the domain [`AgentProvider`] variant-for-variant; this wire twin
/// carries the serialization concerns the domain type must not know about: the
/// lowercase variant tokens (matching the persisted `session.provider` values)
/// and the TypeScript export the UI uses to render the provider badge.
///
/// Also accepted inbound (`Deserialize`) as the optional `provider` selector on
/// a new-session send, so the same token set names a provider in both
/// directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "AgentProvider")]
pub enum WireAgentProvider {
    Claude,
    Codex,
}

impl From<AgentProvider> for WireAgentProvider {
    fn from(provider: AgentProvider) -> Self {
        match provider {
            AgentProvider::Claude => WireAgentProvider::Claude,
            AgentProvider::Codex => WireAgentProvider::Codex,
        }
    }
}

impl From<WireAgentProvider> for AgentProvider {
    fn from(provider: WireAgentProvider) -> Self {
        match provider {
            WireAgentProvider::Claude => AgentProvider::Claude,
            WireAgentProvider::Codex => AgentProvider::Codex,
        }
    }
}
