//! Whether an agent provider can actually be launched on this host.

use crate::AgentProvider;

/// Whether an agent provider can be launched on this host, and why not when it
/// cannot.
///
/// v1 reports **binary presence only**: [`Self::available`] is `false` when the
/// provider's configured launch binary is not resolvable (missing from `PATH`,
/// or an explicit path that does not exist / is not executable). This is what
/// prevents the main accident — picking a provider in the new-session selector
/// whose binary is missing, then hitting a spawn failure.
///
/// The shape deliberately carries a [`Self::detail`] string rather than being a
/// bare `bool` so a future *version-compatibility* verdict (delta's pinned
/// Codex version vs the installed `codex --version`) can slot in without a
/// breaking reshape: an incompatible-but-present binary would still be
/// `available: false` with a `detail` explaining the version gap. That check is
/// deferred to the real-Codex canary, which is where a real pinned version
/// first exists — today the wire protocol is validated against a fake
/// app-server with no version to pin against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAvailability {
    /// Which provider this verdict is about.
    pub provider: AgentProvider,
    /// Whether the provider can be launched on this host right now.
    pub available: bool,
    /// A human-readable reason when [`Self::available`] is `false` (e.g.
    /// "the 'codex' binary for codex was not found on PATH"). `None` when the
    /// provider is available.
    pub detail: Option<String>,
}
