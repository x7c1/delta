//! [`PaneTokenMinter`]: hands out [`PaneToken`]s whose tmux session does not
//! exist yet.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::Result;
use crate::ports::TmuxDriver;
use crate::PaneToken;

/// Mints [`PaneToken`]s as `delta-<n>` with a monotonic counter, skipping every
/// name whose tmux session is still alive.
///
/// The counter is atomic, so two concurrent mints never hand out the same name
/// without any external lock — a fresh spawn mints its token before taking the
/// registry mutex. The counter restarts at `delta-1` on every server start,
/// which is why [`Self::mint_free`] also probes tmux: see there.
#[derive(Debug)]
pub struct PaneTokenMinter {
    next: AtomicU64,
}

impl Default for PaneTokenMinter {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneTokenMinter {
    /// Create a minter starting from `delta-1`.
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Mint the next token whose tmux session does not exist, probing `tmux`
    /// with [`TmuxDriver::has_session`]. A probe failure is propagated.
    ///
    /// The counter resets on each server start, but `delta-<n>` tmux sessions
    /// from a previous run can survive a restart — they are detached, so
    /// stopping the server does not kill them. Creating a tmux session with a
    /// name that already exists fails with "duplicate session", which would
    /// fail the spawn. So any minted name whose tmux session is still alive is
    /// skipped in favour of the next one. The monotonic counter guarantees this
    /// terminates (there are finitely many surviving sessions) and that two
    /// concurrent callers never contend for the same name.
    pub async fn mint_free<T: TmuxDriver + ?Sized>(&self, tmux: &T) -> Result<PaneToken> {
        loop {
            let token = self.mint();
            if !tmux.has_session(token.as_str()).await? {
                return Ok(token);
            }
        }
    }

    /// The next `delta-<n>`, without asking tmux whether it is taken. Private:
    /// every token leaving the minter goes through [`Self::mint_free`].
    fn mint(&self) -> PaneToken {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        PaneToken::minted(n)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::error::Error;
    use crate::interactor::FakeTmux;

    fn tmux_with_live(names: &[&str]) -> FakeTmux {
        FakeTmux {
            live: Mutex::new(names.iter().map(|n| (*n).to_owned()).collect()),
            ..Default::default()
        }
    }

    #[test]
    fn mints_monotonic_unique_tokens() {
        let minter = PaneTokenMinter::new();
        assert_eq!(minter.mint().as_str(), "delta-1");
        assert_eq!(minter.mint().as_str(), "delta-2");
        assert_eq!(minter.mint().as_str(), "delta-3");
    }

    #[test]
    fn default_and_new_start_at_the_same_token() {
        assert_eq!(PaneTokenMinter::default().mint().as_str(), "delta-1");
        assert_eq!(PaneTokenMinter::new().mint().as_str(), "delta-1");
    }

    #[test]
    fn token_is_a_valid_tmux_session_name() {
        // tmux session names must not contain `.` or `:`; `delta-<n>` never does.
        let token = PaneTokenMinter::new().mint();
        assert!(!token.as_str().contains('.'));
        assert!(!token.as_str().contains(':'));
    }

    #[tokio::test]
    async fn mint_free_skips_live_names_and_returns_the_next_free_one() {
        let minter = PaneTokenMinter::new();
        let tmux = tmux_with_live(&["delta-1", "delta-3"]);

        let first = minter.mint_free(&tmux).await.expect("probe succeeds");
        assert_eq!(first.as_str(), "delta-2", "the live delta-1 is skipped");

        let second = minter.mint_free(&tmux).await.expect("probe succeeds");
        assert_eq!(second.as_str(), "delta-4", "the live delta-3 is skipped");
    }

    #[tokio::test]
    async fn mint_free_propagates_a_probe_error() {
        let minter = PaneTokenMinter::new();
        let tmux = FakeTmux::default();
        tmux.fail_probes();

        let err = minter
            .mint_free(&tmux)
            .await
            .expect_err("a failed probe is not read as a free name");
        assert!(matches!(err, Error::Tmux(_)), "got {err:?}");
    }
}
