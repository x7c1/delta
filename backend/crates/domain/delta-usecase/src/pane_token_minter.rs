//! [`PaneTokenMinter`]: hands out collision-free [`PaneToken`]s.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::PaneToken;

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
        PaneToken::minted(n)
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
