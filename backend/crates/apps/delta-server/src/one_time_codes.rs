//! One-time codes: how a page that holds no credentials yet comes to hold the
//! per-run bearer token.
//!
//! The server never puts the token in a page it serves, since the page is
//! served to anyone who asks for it. Instead whoever starts the server — the
//! desktop shell, the `delta-server` binary, `delta-server open-url` — gets a
//! URL carrying a one-time code in its fragment (`/#code=<code>`), and the page
//! trades that code for the token through `POST /api/auth/exchange` (see
//! `crate::api::auth`).
//!
//! A code is:
//!
//! - **unguessable** — minted from the operating system's CSPRNG with the same
//!   244 bits of entropy as the token itself (`config::mint_secret`);
//! - **single-use** — redeeming it removes it, so a second exchange with the
//!   same code is refused;
//! - **short-lived** — it expires [`CODE_LIFETIME`] after it was minted
//!   whether or not it was used;
//! - **compared in constant time** — every outstanding code is compared in
//!   full, so the time a redemption takes says nothing about which bytes of a
//!   guess matched.
//!
//! At most [`MAX_OUTSTANDING`] codes wait to be redeemed; minting past that
//! drops the oldest, so the set cannot grow without bound.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::auth_guard::constant_time_eq;
use crate::config::mint_secret;

/// How long a code stays redeemable after it was minted.
pub const CODE_LIFETIME: Duration = Duration::from_secs(10 * 60);

/// How many unredeemed codes are kept at once.
const MAX_OUTSTANDING: usize = 32;

/// The codes minted and not yet redeemed or expired.
#[derive(Debug)]
pub(crate) struct OneTimeCodes {
    lifetime: Duration,
    /// Each outstanding code with the moment it stops being redeemable, oldest
    /// first.
    outstanding: Mutex<Vec<(String, Instant)>>,
}

impl OneTimeCodes {
    /// An empty set whose codes are redeemable for `lifetime`.
    pub(crate) fn new(lifetime: Duration) -> Self {
        Self {
            lifetime,
            outstanding: Mutex::new(Vec::new()),
        }
    }

    /// Mint a fresh code, redeemable once within the lifetime.
    pub(crate) fn mint(&self) -> String {
        self.mint_at(Instant::now())
    }

    /// Redeem `code`: true, and the code is gone, when it was outstanding and
    /// has not expired; false for a used, expired or unknown code.
    pub(crate) fn redeem(&self, code: &str) -> bool {
        self.redeem_at(code, Instant::now())
    }

    fn mint_at(&self, now: Instant) -> String {
        let code = mint_secret();
        let mut outstanding = self.lock();
        outstanding.retain(|(_, expires)| *expires > now);
        if outstanding.len() >= MAX_OUTSTANDING {
            outstanding.remove(0);
        }
        outstanding.push((code.clone(), now + self.lifetime));
        code
    }

    fn redeem_at(&self, code: &str, now: Instant) -> bool {
        let mut outstanding = self.lock();
        // Compare against every entry, without stopping at a match, so the
        // time taken does not depend on where (or whether) the code matched.
        let mut matched = None;
        for (index, (candidate, _)) in outstanding.iter().enumerate() {
            if constant_time_eq(candidate.as_bytes(), code.as_bytes()) {
                matched = Some(index);
            }
        }
        let Some(index) = matched else {
            return false;
        };
        let (_, expires) = outstanding.remove(index);
        expires > now
    }

    /// The outstanding codes. A panic while the lock was held cannot leave the
    /// list half-updated (every update is a single `Vec` operation), so a
    /// poisoned lock is taken over rather than propagated.
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<(String, Instant)>> {
        self.outstanding
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_redeemed_once() {
        let codes = OneTimeCodes::new(CODE_LIFETIME);
        let code = codes.mint();
        assert!(codes.redeem(&code));
        assert!(!codes.redeem(&code), "a used code is refused");
    }

    #[test]
    fn an_unknown_or_empty_code_is_refused() {
        let codes = OneTimeCodes::new(CODE_LIFETIME);
        let code = codes.mint();
        assert!(!codes.redeem("not-a-code"));
        assert!(!codes.redeem(""));
        assert!(
            codes.redeem(&code),
            "a refused guess does not consume a code"
        );
    }

    #[test]
    fn a_code_expires_after_its_lifetime() {
        let codes = OneTimeCodes::new(CODE_LIFETIME);
        let minted = Instant::now();
        let code = codes.mint_at(minted);
        assert!(!codes.redeem_at(&code, minted + CODE_LIFETIME));

        let code = codes.mint_at(minted);
        assert!(codes.redeem_at(&code, minted + CODE_LIFETIME - Duration::from_secs(1)));
    }

    #[test]
    fn codes_are_distinct_and_long() {
        let codes = OneTimeCodes::new(CODE_LIFETIME);
        let first = codes.mint();
        let second = codes.mint();
        assert_ne!(first, second);
        // 64 hex characters: well above 128 bits of entropy.
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn minting_past_the_cap_drops_the_oldest() {
        let codes = OneTimeCodes::new(CODE_LIFETIME);
        let oldest = codes.mint();
        let rest: Vec<String> = (1..=MAX_OUTSTANDING).map(|_| codes.mint()).collect();
        assert!(!codes.redeem(&oldest));
        assert!(rest.iter().all(|code| codes.redeem(code)));
    }
}
