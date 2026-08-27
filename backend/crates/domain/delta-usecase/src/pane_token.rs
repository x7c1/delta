//! [`PaneToken`]: a Delta-minted tmux session name, decoupled from Claude's
//! `session_id`.
//!
//! Every tmux session Delta drives is named after a `PaneToken`, never after
//! Claude's `session_id`. This decoupling is what lets a closed conversation be
//! resumed under a fresh tmux session (`claude --resume <id>`) without the new
//! tmux session's name colliding with any existing one: the name is a token
//! Delta owns, not the conversation id.
//!
//! Tokens are minted by [`PaneTokenMinter`], which hands out `delta-<n>` for a
//! monotonically increasing `n`. The counter is atomic, so minting is
//! collision-free without relying on randomness or wall-clock seeds.
//! `delta-<n>` is always a valid tmux session name (no `.` or `:`).

use std::sync::atomic::{AtomicU64, Ordering};

/// A Delta-minted tmux session name, e.g. `delta-1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PaneToken(String);

impl PaneToken {
    /// The token as a tmux session name.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The launch key of an **adapter-backed** (terminal-less) session, derived
    /// from its session id: `adapter-<session-id>`.
    ///
    /// Such a session has no tmux pane at all, so this value is **never handed
    /// to tmux** — no `new-session`, no `has-session`, no `kill-session` is ever
    /// issued for it. It exists only because the accept→launch window is keyed
    /// by [`PaneToken`] (`LaunchingSpawn`, `finish_launch`), and an
    /// adapter-backed launch has to live in that same window so both providers
    /// share one launch shell. Deriving it from the session id (rather than
    /// minting one) keeps [`PaneTokenMinter`]'s counter — and the `delta-<n>`
    /// namespace tmux really uses — untouched, and makes the key both unique
    /// and reproducible without probing tmux for a free name.
    ///
    /// The `adapter-` prefix is what keeps it out of that namespace: a token
    /// that reached tmux by mistake could never collide with a real pane.
    pub(crate) fn for_adapter_launch(session_id: &delta_model::SessionId) -> Self {
        Self(format!("adapter-{}", session_id.as_str()))
    }

    /// Construct a token from a raw session name, for tests that seed the
    /// registry directly (the watchdog tests push a pending spawn whose token a
    /// production minter would otherwise own).
    #[cfg(test)]
    pub(crate) fn from_raw(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl std::fmt::Display for PaneToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Mints unique [`PaneToken`]s as `delta-<n>` with a monotonic counter.
///
/// The counter is atomic, so minting is collision-free without any external
/// lock — a fresh spawn mints its token before taking the registry mutex.
#[derive(Debug, Default)]
pub struct PaneTokenMinter {
    next: AtomicU64,
}

impl PaneTokenMinter {
    /// Create a minter starting from `delta-1`.
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Mint the next unique token.
    pub fn mint(&self) -> PaneToken {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        PaneToken(format!("delta-{n}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_monotonic_unique_tokens() {
        let minter = PaneTokenMinter::new();
        assert_eq!(minter.mint().as_str(), "delta-1");
        assert_eq!(minter.mint().as_str(), "delta-2");
        assert_eq!(minter.mint().as_str(), "delta-3");
    }

    #[test]
    fn adapter_launch_key_is_derived_from_the_session_id_and_never_minted() {
        let minter = PaneTokenMinter::new();
        let session_id = delta_model::SessionId::from("sess-1");
        let key = PaneToken::for_adapter_launch(&session_id);
        assert_eq!(key.as_str(), "adapter-sess-1");
        // It is reproducible from the id alone…
        assert_eq!(key, PaneToken::for_adapter_launch(&session_id));
        // …and costs the tmux namespace nothing: the minter is untouched, and
        // the key could not collide with a `delta-<n>` pane even if it leaked.
        assert_eq!(minter.mint().as_str(), "delta-1");
        assert!(!key.as_str().starts_with("delta-"));
    }

    #[test]
    fn token_is_a_valid_tmux_session_name() {
        // tmux session names must not contain `.` or `:`; `delta-<n>` never does.
        let token = PaneTokenMinter::new().mint();
        assert!(!token.as_str().contains('.'));
        assert!(!token.as_str().contains(':'));
    }
}
