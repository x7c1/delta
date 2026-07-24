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

use crate::agent::{AgentAdapter, AgentCapabilities, AgentProvider};
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

    /// Stand up the backing connection and return a live adapter.
    ///
    /// Performs the provider's spawn/handshake, so it is called lazily when a
    /// session needs the provider — never at startup. Fails if the provider's
    /// binary is unavailable or the handshake does not complete.
    async fn connect(&self) -> Result<Arc<dyn AgentAdapter>>;
}
