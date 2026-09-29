//! [`NullLaunchOptionVocabulary`]: the permissive default
//! [`LaunchOptionVocabulary`].

use crate::agent::{AgentProvider, LaunchOptionCardinality, LaunchOptionVocabulary};

/// The default vocabulary: nothing is dangerous, and every name may repeat.
///
/// Wired by [`crate::Interactor::new`] so a configuration that has not injected
/// the real vocabulary (the domain's own tests, dev harnesses) behaves exactly as
/// it did before these rules existed rather than refusing writes or launches it
/// cannot classify. Both answers are deliberately the *permissive* ones,
/// mirroring [`AgentAdapterFactory::validate_launch_options`]: the danger rule
/// names a closed set of known-dangerous spellings, so a stub that guessed
/// "dangerous" would have to reject everything; and a stub that guessed
/// [`LaunchOptionCardinality::Single`] would group — and refuse to combine —
/// rows it knows nothing about. [`LaunchOptionCardinality::Multiple`] groups
/// nothing and rejects nothing. Production wiring always installs the real
/// vocabulary through [`crate::Interactor::with_launch_option_vocabulary`], and
/// guard tests in the composition root pin that no shipped preset is dangerous
/// and that the shipped model presets form one choice group.
///
/// [`AgentAdapterFactory::validate_launch_options`]: crate::agent::AgentAdapterFactory::validate_launch_options
pub struct NullLaunchOptionVocabulary;

impl LaunchOptionVocabulary for NullLaunchOptionVocabulary {
    fn is_dangerous(&self, _provider: AgentProvider, _name: &str, _value: Option<&str>) -> bool {
        false
    }

    fn cardinality(&self, _provider: AgentProvider, _name: &str) -> LaunchOptionCardinality {
        LaunchOptionCardinality::Multiple
    }
}
