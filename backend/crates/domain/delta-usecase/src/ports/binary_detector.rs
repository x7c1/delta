//! Detecting whether a provider's launch binary is present on this host.
//!
//! The new-session provider selector needs to know, before a user picks a
//! provider, whether that provider's binary can actually be launched — so an
//! un-installed provider can be disabled with a reason instead of failing at
//! spawn time. This port abstracts the "is this binary resolvable" check so the
//! use case stays pure and tests can supply present/absent verdicts without
//! touching the real filesystem, mirroring how [`GhCli`](crate::ports::GhCli)
//! abstracts the `gh` probe.

use async_trait::async_trait;

/// Reports whether a launch binary is resolvable on this host.
///
/// The gateway resolves `bin` the same way spawn's `Command::new(bin)` would:
/// a bare command name is looked up on `PATH`; an explicit path (absolute or
/// containing a separator) is checked for existence and execute permission. A
/// missing binary is reported as `false` rather than surfacing an error, so the
/// availability endpoint always answers rather than 5xx-ing on a host that
/// happens to be missing a provider.
///
/// Implementations are expected to be cheap and deterministic-friendly for
/// tests. The production gateway memoises per-binary for the process lifetime
/// (like the `gh auth status` probe): binary presence effectively does not
/// change while the server runs, so a host without a provider answers cheaply.
#[async_trait]
pub trait BinaryDetector: Send + Sync {
    /// Whether `bin` is resolvable on this host.
    ///
    /// `bin` is used exactly as spawn would use it as `argv[0]` — a bare name
    /// (resolved via `PATH`) or an explicit path — so availability matches what
    /// a real launch attempt would resolve.
    async fn is_available(&self, bin: &str) -> bool;
}
