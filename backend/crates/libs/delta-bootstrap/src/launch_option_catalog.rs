//! The launch options Delta ships, per provider and all together.

use claude_agent::CLAUDE_LAUNCH_OPTION_CATALOG;
use codex_agent::CODEX_LAUNCH_OPTION_CATALOG;
use delta_usecase::{AgentProvider, LaunchOptionPreset};

/// The launch options Delta *ships* for a provider, resolved the same way as
/// [`provider_capabilities`].
///
/// Each provider's catalog is declared in its own gateway adapter, beside the
/// `launch_option_style` capability that says which vocabulary the entries'
/// `name` is read in — the same rule that keeps the capability profile in the
/// adapter that owns the behaviour. This accessor is the one place that knows
/// every catalog, so the composition root can materialize all of them into the
/// registry at startup without any layer above the gateways naming a provider.
///
/// An exhaustive `match`, deliberately: a new provider has to decide here what
/// it ships (an empty slice is a perfectly good answer) rather than silently
/// shipping nothing.
///
/// [`provider_capabilities`]: crate::provider_capabilities()
pub fn launch_option_catalog(provider: AgentProvider) -> &'static [LaunchOptionPreset] {
    match provider {
        AgentProvider::Claude => CLAUDE_LAUNCH_OPTION_CATALOG,
        AgentProvider::Codex => CODEX_LAUNCH_OPTION_CATALOG,
    }
}

/// Every provider's shipped launch options, concatenated.
///
/// Reconciliation takes the whole declared set at once: its closing sweep —
/// "drop the shipped rows the catalog no longer declares" — is registry-wide,
/// so feeding it one provider at a time would make each pass retire the other
/// provider's rows.
///
/// Public because it is also the honest expectation for a test that asserts what
/// a freshly-built store holds: the count comes from the catalogs themselves, so
/// adding a preset does not break an unrelated assertion.
pub fn all_launch_option_presets() -> Vec<LaunchOptionPreset> {
    AgentProvider::ALL
        .iter()
        .flat_map(|provider| launch_option_catalog(*provider).iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every provider's catalog reaches the concatenation, declares only
    /// entries for the provider it is registered under, and contributes no
    /// duplicate key.
    ///
    /// The only place all the catalogs are visible at once, so it is where
    /// those three are checked: the key is the reconciliation identity, so two
    /// catalogs colliding on one would collapse into a single registry row, and
    /// a stray provider would put a CLI flag in front of a Codex picker (or a
    /// `thread/start` field in front of a Claude one). Each adapter's own tests
    /// cover only what is specific to its provider's vocabulary.
    #[test]
    fn every_providers_catalog_is_declared_with_a_unique_key() {
        let presets = all_launch_option_presets();
        for provider in AgentProvider::ALL {
            let declared = launch_option_catalog(provider);
            assert!(
                declared
                    .iter()
                    .all(|preset| presets.iter().any(|each| each.key == preset.key)),
                "{}'s catalog is missing from the concatenation",
                provider.as_str()
            );
            assert!(
                declared.iter().all(|preset| preset.provider == provider),
                "{}'s catalog declares an entry for another provider",
                provider.as_str()
            );
        }

        let mut keys: Vec<&str> = presets.iter().map(|preset| preset.key).collect();
        let declared = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            declared,
            "two shipped launch options share a key: {keys:?}"
        );
    }
}
