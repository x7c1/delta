//! Which Claude launch-option flags may repeat on one command line.
//!
//! Its own module, beside the declared catalog and the danger predicate, for the
//! same reason they are here: the adapter that owns Claude owns Claude's
//! vocabulary. The composition root reads this through one per-provider accessor
//! and hands it to the domain as a [`LaunchOptionVocabulary`], which is what
//! turns the registry rows of one single-valued flag into an exclusive choice
//! group — a radio group in the picker, one default in the registry, and a
//! refused launch when two of them are selected.
//!
//! **The default is [`LaunchOptionCardinality::Single`]**, and a closed list of
//! repeatable flags answers [`LaunchOptionCardinality::Multiple`]. The two
//! failure modes are not symmetric. A repeatable flag wrongly classified single
//! shows up at once, as a picker that will not let the user tick two of its
//! rows, and is fixed by adding the flag here. A `Multiple` default would
//! instead have to enumerate every single-valued flag the CLI has, would lag
//! behind every new one, and would fail silently — two values on one command
//! line, the outcome left to the CLI.
//!
//! The list is a snapshot of the upstream CLI's `claude --help` (the variadic
//! `<...>` flags and the ones documented as repeatable), like the danger
//! spellings: when the CLI grows a repeatable flag, it is added here.
//!
//! [`LaunchOptionVocabulary`]: delta_usecase::LaunchOptionVocabulary

use delta_usecase::LaunchOptionCardinality;

/// The `claude` flags that accept several values, so several registry rows of
/// one of them may be selected together.
///
/// Both spellings of the tool-list flags are listed: the CLI accepts the
/// camel-case and the kebab-case form alike.
const REPEATABLE_FLAGS: &[&str] = &[
    // Variadic `<...>` arguments.
    "--add-dir",
    "--allowedTools",
    "--allowed-tools",
    "--disallowedTools",
    "--disallowed-tools",
    "--betas",
    "--file",
    "--mcp-config",
    "--tools",
    // Documented as "(repeatable: ...)".
    "--plugin-dir",
    "--plugin-url",
];

/// Whether a Claude launch-option flag takes one value per session or may
/// repeat.
///
/// `name` is the CLI flag exactly as the registry stores it.
pub fn launch_option_cardinality(name: &str) -> LaunchOptionCardinality {
    if REPEATABLE_FLAGS.contains(&name) {
        LaunchOptionCardinality::Multiple
    } else {
        LaunchOptionCardinality::Single
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_is_single_valued() {
        assert_eq!(
            launch_option_cardinality("--model"),
            LaunchOptionCardinality::Single
        );
    }

    #[test]
    fn add_dir_and_plugin_dir_repeat() {
        assert_eq!(
            launch_option_cardinality("--add-dir"),
            LaunchOptionCardinality::Multiple
        );
        assert_eq!(
            launch_option_cardinality("--plugin-dir"),
            LaunchOptionCardinality::Multiple
        );
    }

    /// A flag this list does not know takes the single-valued default.
    #[test]
    fn an_unknown_flag_is_single_valued() {
        assert_eq!(
            launch_option_cardinality("--some-future-flag"),
            LaunchOptionCardinality::Single
        );
    }
}
