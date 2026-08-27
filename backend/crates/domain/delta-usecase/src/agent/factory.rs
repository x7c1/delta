//! [`AgentAdapterFactory`]: a lazily-connected source of an [`AgentAdapter`].
//!
//! Some providers cannot be turned into a live [`AgentAdapter`] without a
//! side effect at construction time — Codex, for instance, must spawn a
//! `codex app-server` process and complete its `initialize` handshake before
//! its adapter exists. Doing that eagerly at startup would make a machine
//! without the provider installed fail to boot, so the composition root holds
//! this factory (which carries only launch configuration) instead of a live
//! adapter, and defers [`Self::connect`] to the moment a session actually
//! needs the provider.
//!
//! Providers whose adapter is cheap to build (no process, no handshake) do not
//! need a factory; the core can hold their [`AgentAdapter`] directly. This
//! trait exists for the ones that do.

use std::sync::Arc;

use async_trait::async_trait;

use crate::agent::{AgentAdapter, AgentCapabilities, AgentProvider, LaunchOptionSpec};
use crate::error::Result;

/// Builds a live [`AgentAdapter`] on demand, deferring any process spawn or
/// network handshake out of the startup path.
///
/// The composition root wires a concrete factory holding the provider's launch
/// configuration without touching the provider's binary; the backing
/// connection is stood up only when [`Self::connect`] is first called.
#[async_trait]
pub trait AgentAdapterFactory: Send + Sync {
    /// Which provider the built adapter drives.
    fn provider(&self) -> AgentProvider;

    /// The provider's static capability profile, resolved *without* connecting.
    ///
    /// Returns the same profile the built adapter's
    /// [`AgentAdapter::capabilities`] reports (both read one declaration in the
    /// gateway layer), so dispatch decisions made before [`Self::connect`] —
    /// notably whether a session is adapter-backed at all — can never drift
    /// from what a running adapter would say. No process is spawned here.
    fn capabilities(&self) -> AgentCapabilities;

    /// Decide, *without* connecting, whether the launch options a fresh session
    /// selected are ones this provider can be launched with.
    ///
    /// Rendering the user's selections into the provider's launch shape is the
    /// adapter's job, and some selections are refused there rather than applied
    /// (Codex: a `thread/start` field Delta fills in itself, the same field
    /// selected twice, two `config` rows that disagree — all
    /// [`Error::LaunchOptionRejected`]). That refusal is a **property of the
    /// request**: it depends only on the launch directory, the selected options
    /// and whether the directory is a Delta-created worktree, never on a live
    /// connection or on anything the provider says. Exposing it here is what
    /// lets the accept phase of a spawn — which runs inside `POST /api/sends`,
    /// before any row exists and long before the background launch connects —
    /// answer such a selection with a synchronous `400` the composer shows on
    /// the failed send row, instead of accepting the send and surfacing the
    /// mistake later as an asynchronous `spawn_failed`.
    ///
    /// It is a *pre*-check, not the check: the launch still renders the options
    /// for real, from the same builder, so the two cannot drift and this method
    /// never becomes a second place the rules are written down.
    ///
    /// `workdir` / `worktree_repo_root` are the very values the launch will
    /// carry on its [`LaunchRequest`](crate::agent::LaunchRequest) (the planned
    /// launch directory and, for a worktree spawn, the repository it is cut
    /// from), so what is validated here is exactly what will be rendered.
    ///
    /// The default accepts everything: a provider whose adapter can refuse no
    /// selection (Claude renders each option as argv, where nothing is
    /// rejectable) needs no pre-check, and neither do the test fakes.
    ///
    /// [`Error::LaunchOptionRejected`]: crate::error::Error::LaunchOptionRejected
    fn validate_launch_options(
        &self,
        _workdir: &str,
        _options: &[LaunchOptionSpec],
        _worktree_repo_root: Option<&str>,
    ) -> Result<()> {
        Ok(())
    }

    /// Stand up the backing connection and return a live adapter.
    ///
    /// Performs the provider's spawn/handshake, so it is called lazily when a
    /// session needs the provider — never at startup. Fails if the provider's
    /// binary is unavailable or the handshake does not complete.
    async fn connect(&self) -> Result<Arc<dyn AgentAdapter>>;
}
