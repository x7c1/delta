//! The composition root's side of the launch-option danger rule: the
//! per-provider gateway predicates, and the [`LaunchOptionDangerPolicy`] the
//! domain is wired with.
//!
//! Its own module — named the same as the gateway modules it dispatches to and
//! the domain port it implements — so the dispatch, the port adapter and the
//! guard test over the shipped catalogs sit together instead of thickening the
//! crate root.

use delta_usecase::{AgentProvider, LaunchOptionDangerPolicy};

/// Whether a launch option switches `provider`'s own safety mechanisms off,
/// resolved the same way as [`provider_capabilities`].
///
/// Each provider's predicate is declared in its own gateway adapter, beside the
/// catalog it ships, because the spellings that mean "stop asking" are that
/// provider's vocabulary — `--dangerously-skip-permissions` for Claude, a
/// `danger-full-access` sandbox for Codex. This accessor is the one place that
/// knows every predicate.
///
/// An exhaustive `match`, deliberately: a new provider has to state which of its
/// options disarm it (`|_| false` is a fine answer for a provider that has none)
/// rather than silently shipping "nothing here is dangerous".
///
/// [`provider_capabilities`]: crate::provider_capabilities
pub fn is_launch_option_dangerous(
    provider: AgentProvider,
    name: &str,
    value: Option<&str>,
) -> bool {
    match provider {
        AgentProvider::Claude => claude_agent::is_dangerous_launch_option(name, value),
        AgentProvider::Codex => codex_agent::is_dangerous_launch_option(name, value),
    }
}

/// The [`LaunchOptionDangerPolicy`] the domain is wired with: the gateway
/// predicates above, behind the port.
///
/// A zero-sized adapter rather than a closure so the wiring reads as one named
/// thing in [`build`], and so the `match` stays in
/// [`is_launch_option_dangerous`] where a new provider is forced to face it.
///
/// [`build`]: crate::build
pub struct GatewayLaunchOptionDanger;

impl LaunchOptionDangerPolicy for GatewayLaunchOptionDanger {
    fn is_dangerous(&self, provider: AgentProvider, name: &str, value: Option<&str>) -> bool {
        is_launch_option_dangerous(provider, name, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::all_launch_option_presets;

    /// No launch option Delta *ships* may be one that disarms the agent.
    ///
    /// This is the guard that keeps the no-silent-default rule whole. Startup
    /// reconciliation upserts every preset while **preserving**
    /// `default_enabled`, so it is the one writer that can put a
    /// `default_enabled` row in front of the registry's own refusals — a
    /// dangerous preset shipped by mistake could therefore resurrect a
    /// pre-checked bypass on every boot, past both the create and the `PATCH`
    /// guard. Checked in this crate rather than in either catalog because this is
    /// the one place both the catalogs and the wired predicate are visible at
    /// once.
    #[test]
    fn no_shipped_preset_is_dangerous() {
        for preset in all_launch_option_presets() {
            assert!(
                !is_launch_option_dangerous(preset.provider, preset.name, preset.value),
                "shipped launch option `{}` ({} = {:?}) disables the agent's own \
                 safety mechanism, so it must not be shipped: reconciliation \
                 preserves `default_enabled`, which would let it be pre-checked \
                 on every new session",
                preset.key,
                preset.name,
                preset.value
            );
        }
    }
}
