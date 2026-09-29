//! [`LaunchOptionVocabulary`]: how a launch option reads in its provider's own
//! vocabulary.
//!
//! A launch option is otherwise a pass-through (see [`delta_model`]'s
//! `launch_option` module): Delta does not read the names or the values, because
//! the agent that receives them owns that vocabulary. This port is where the
//! few classifications that *do* need that vocabulary reach the domain:
//!
//! - **Danger.** A specific handful of values do not configure the agent so much
//!   as disable its guardrails — Claude's `--dangerously-skip-permissions`,
//!   Codex's `danger-full-access` sandbox. Such an option stays usable, but it
//!   must never be silent and never be on by default.
//! - **Cardinality.** Registry rows that share a `(provider, name)` are
//!   candidate values of one setting. Whether that setting takes one value
//!   (Claude's `--model`) or may repeat (Claude's `--add-dir`, Codex's `config`)
//!   is the provider's vocabulary too; a single-valued name turns its rows into
//!   an exclusive choice group, enforced at launch and on the one-default rule
//!   of the registry.
//!
//! So the vocabulary stays in the gateway layer and reaches the domain through
//! this port, the same way [`AgentAdapterFactory::validate_launch_options`] does
//! for the selections an adapter refuses. Unlike that one it cannot hang off the
//! adapter factory: Claude registers no factory at all (its sessions take the
//! native PTY path), and Claude is exactly the provider with the loudest
//! dangerous flag and the most single-valued ones. So the port is a single
//! object covering every provider, wired once by the composition root — which
//! is already the one layer that knows every gateway adapter, and already
//! dispatches `provider_capabilities` and `launch_option_catalog` per provider
//! the same way. The next classification that needs the provider's vocabulary
//! belongs here as well.
//!
//! Every answer is derived, never stored: it is asked afresh on each write and
//! on each response, so a vocabulary update in a gateway reclassifies the rows
//! already in the registry without a migration.
//!
//! [`AgentAdapterFactory::validate_launch_options`]: crate::agent::AgentAdapterFactory::validate_launch_options

use crate::agent::{AgentProvider, LaunchOptionCardinality};

/// Classifies a registered launch option in its provider's vocabulary.
///
/// Consulted on both write paths of the launch-option registry (a create that
/// asks for `default_enabled`, and a `PATCH` that turns it on), on the read
/// path, where the answers ride out on the wire so the browser can mark a
/// dangerous row and group the rows of one single-valued name, and on both
/// spawn paths, which refuse two selected values of one single-valued name.
pub trait LaunchOptionVocabulary: Send + Sync {
    /// Whether `(name, value)`, read in `provider`'s vocabulary, turns off a
    /// safety mechanism of that agent.
    ///
    /// The predicate takes the registry pair verbatim — `value` `None` for a
    /// valueless option — because dangerousness is a property of the pair:
    /// `--permission-mode` is benign except for one value, and
    /// `--dangerously-skip-permissions` is dangerous whatever value it carries.
    fn is_dangerous(&self, provider: AgentProvider, name: &str, value: Option<&str>) -> bool;

    /// Whether `name`, read in `provider`'s vocabulary, takes one value per
    /// session or may repeat.
    ///
    /// A property of the name alone: every row registered under a
    /// [`LaunchOptionCardinality::Single`] name is one candidate value of the
    /// same setting, whatever value it carries.
    fn cardinality(&self, provider: AgentProvider, name: &str) -> LaunchOptionCardinality;
}
