//! Which Codex launch options may be selected more than once.
//!
//! Its own module, beside the declared catalog and the danger predicate, because
//! the adapter that owns Codex owns Codex's vocabulary. The composition root
//! reads this through one per-provider accessor and hands it to the domain as a
//! [`LaunchOptionVocabulary`], which turns the rows of one single-valued name
//! into an exclusive choice group — a radio group in the picker, one default in
//! the registry, and a refused launch when two of them are selected.
//!
//! A Codex launch option's `name` is a `thread/start` field, and a JSON field can
//! be set once, so every name is [`LaunchOptionCardinality::Single`] — except
//! [`CONFIG_FIELD`], which is not one setting but an object holding many, and
//! which [`thread_start_params`] deep-merges across selections. That builder
//! consults this same function for its duplicate-field check, so the rule the
//! domain enforces and the rule the builder enforces cannot drift apart; the
//! builder still owns the `config` merge-conflict detection.
//!
//! [`LaunchOptionVocabulary`]: delta_usecase::LaunchOptionVocabulary
//! [`thread_start_params`]: super::thread_start_params

use delta_usecase::LaunchOptionCardinality;

use super::CONFIG_FIELD;

/// Whether a Codex launch option — a `thread/start` field name — takes one value
/// per session or may be selected several times.
pub fn launch_option_cardinality(name: &str) -> LaunchOptionCardinality {
    if name == CONFIG_FIELD {
        LaunchOptionCardinality::Multiple
    } else {
        LaunchOptionCardinality::Single
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_may_repeat() {
        assert_eq!(
            launch_option_cardinality(CONFIG_FIELD),
            LaunchOptionCardinality::Multiple
        );
    }

    #[test]
    fn model_is_single_valued() {
        assert_eq!(
            launch_option_cardinality("model"),
            LaunchOptionCardinality::Single
        );
    }
}
