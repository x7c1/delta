//! The composition root's side of the launch-option vocabulary: the
//! per-provider gateway classifications (danger, cardinality), and the
//! [`LaunchOptionVocabulary`] the domain is wired with.
//!
//! Its own module — named for the domain port it implements — so the dispatch,
//! the port adapter and the guard tests over the shipped catalogs sit together
//! instead of thickening the crate root.

use delta_usecase::{AgentProvider, LaunchOptionCardinality, LaunchOptionVocabulary};

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
/// [`provider_capabilities`]: crate::provider_capabilities()
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

/// Whether a launch-option `name` takes one value per session or may repeat,
/// in `provider`'s vocabulary, resolved the same way as
/// [`is_launch_option_dangerous`].
///
/// Each provider decides its own default beside its catalog — Claude answers
/// single-valued unless the flag is on its repeatable list, Codex answers
/// single-valued for every `thread/start` field but `config`. An exhaustive
/// `match` for the same reason as the danger accessor: a new provider has to
/// state its rule.
pub fn launch_option_cardinality(provider: AgentProvider, name: &str) -> LaunchOptionCardinality {
    match provider {
        AgentProvider::Claude => claude_agent::launch_option_cardinality(name),
        AgentProvider::Codex => codex_agent::launch_option_cardinality(name),
    }
}

/// The [`LaunchOptionVocabulary`] the domain is wired with: the gateway
/// classifications above, behind the port.
///
/// A zero-sized adapter rather than closures so the wiring reads as one named
/// thing in [`build`], and so each `match` stays in its accessor where a new
/// provider is forced to face it.
///
/// [`build`]: crate::build()
pub struct GatewayLaunchOptionVocabulary;

impl LaunchOptionVocabulary for GatewayLaunchOptionVocabulary {
    fn is_dangerous(&self, provider: AgentProvider, name: &str, value: Option<&str>) -> bool {
        is_launch_option_dangerous(provider, name, value)
    }

    fn cardinality(&self, provider: AgentProvider, name: &str) -> LaunchOptionCardinality {
        launch_option_cardinality(provider, name)
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

    /// Every `--model` row Delta ships is one value of a single-valued setting.
    ///
    /// The shipped model presets exist to be a radio group — one row per model
    /// family, of which a session takes at most one — so a vocabulary change
    /// that made `--model` repeatable would silently turn them back into
    /// independent checkboxes that put the flag on the command line twice.
    #[test]
    fn every_shipped_model_preset_is_single_valued() {
        let models: Vec<_> = all_launch_option_presets()
            .into_iter()
            .filter(|preset| preset.name == "--model")
            .collect();
        assert!(!models.is_empty(), "the catalogs ship `--model` presets");
        for preset in models {
            assert_eq!(
                launch_option_cardinality(preset.provider, preset.name),
                LaunchOptionCardinality::Single,
                "shipped launch option `{}` must form a choice group",
                preset.key
            );
        }
    }

    /// The shipped Codex `config` preset stays an independent option: `config`
    /// rows are deep-merged, so grouping them would forbid a combination the
    /// adapter supports.
    #[test]
    fn the_shipped_codex_config_preset_may_repeat() {
        let preset = all_launch_option_presets()
            .into_iter()
            .find(|preset| preset.key == "codex:config-reasoning-summary")
            .expect("the Codex catalog ships `codex:config-reasoning-summary`");
        assert_eq!(
            launch_option_cardinality(preset.provider, preset.name),
            LaunchOptionCardinality::Multiple
        );
    }
}
