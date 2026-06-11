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
    fn token_is_a_valid_tmux_session_name() {
        // tmux session names must not contain `.` or `:`; `delta-<n>` never does.
        let token = PaneTokenMinter::new().mint();
        assert!(!token.as_str().contains('.'));
        assert!(!token.as_str().contains(':'));
    }
}
