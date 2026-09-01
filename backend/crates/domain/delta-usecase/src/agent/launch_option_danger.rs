//! [`LaunchOptionDangerPolicy`]: which launch options switch the agent's own
//! safety mechanisms off.
//!
//! A launch option is otherwise a pass-through (see [`delta_model`]'s
//! `launch_option` module): Delta does not read the names or the values, because
//! the agent that receives them owns that vocabulary. This is the one exception,
//! and it exists because a *specific* handful of those values do not configure
//! the agent so much as disable its guardrails — Claude's
//! `--dangerously-skip-permissions`, Codex's `danger-full-access` sandbox. Such
//! an option stays usable, but it must never be silent and never be on by
//! default, and deciding which options those are needs the provider's
//! vocabulary.
//!
//! So the vocabulary stays in the gateway layer and reaches the domain through
//! this port, the same way [`AgentAdapterFactory::validate_launch_options`] does
//! for the selections an adapter refuses. Unlike that one it cannot hang off the
//! adapter factory: Claude registers no factory at all (its sessions take the
//! native PTY path), and Claude is exactly the provider with the loudest
//! dangerous flag. So the port is a single object covering every provider, wired
//! once by the composition root — which is already the one layer that knows
//! every gateway adapter, and already dispatches `provider_capabilities` and
//! `launch_option_catalog` per provider the same way.
//!
//! [`AgentAdapterFactory::validate_launch_options`]: crate::agent::AgentAdapterFactory::validate_launch_options

use crate::agent::AgentProvider;

/// Decides whether a registered launch option disables the agent's own safety
/// mechanisms.
///
/// Consulted on both write paths of the launch-option registry (a create that
/// asks for `default_enabled`, and a `PATCH` that turns it on) and on the read
/// path, where the answer rides out on the wire so the browser can mark the row
/// and refuse to pre-check it.
///
/// The predicate takes the registry pair verbatim — `(name, value)`, with
/// `value` `None` for a valueless option — because dangerousness is a property
/// of the pair: `--permission-mode` is benign except for one value, and
/// `--dangerously-skip-permissions` is dangerous whatever value it carries.
pub trait LaunchOptionDangerPolicy: Send + Sync {
    /// Whether `(name, value)`, read in `provider`'s vocabulary, turns off a
    /// safety mechanism of that agent.
    fn is_dangerous(&self, provider: AgentProvider, name: &str, value: Option<&str>) -> bool;
}

/// The default policy: nothing is dangerous.
///
/// Wired by [`crate::Interactor::new`] so a configuration that has not injected
/// the real policy (the domain's own tests, dev harnesses) behaves exactly as it
/// did before this rule existed rather than refusing writes it cannot classify.
/// That is deliberately the *permissive* default, mirroring
/// [`AgentAdapterFactory::validate_launch_options`]: the policy names a closed
/// set of known-dangerous spellings, so a stub that guessed "dangerous" would
/// have to reject everything. Production wiring always installs the real policy
/// through [`crate::Interactor::with_launch_option_danger_policy`], and a guard
/// test in the composition root pins that no shipped preset is dangerous.
///
/// [`AgentAdapterFactory::validate_launch_options`]: crate::agent::AgentAdapterFactory::validate_launch_options
pub struct NoDangerousLaunchOptions;

impl LaunchOptionDangerPolicy for NoDangerousLaunchOptions {
    fn is_dangerous(&self, _provider: AgentProvider, _name: &str, _value: Option<&str>) -> bool {
        false
    }
}
