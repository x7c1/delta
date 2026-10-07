//! Noticing that a newer release of Delta is out.
//!
//! [`ReleaseCheck`] asks a [`ReleaseFeed`] for the newest published release,
//! judges the answer against this build's version, and holds the last result
//! for the browser to read. It only *notices*: nothing here downloads or
//! replaces anything.
//!
//! # What counts as newer
//!
//! The release's tag must be `v<semver>`, and only a version strictly greater
//! than this build's by SemVer precedence counts. Precedence ignores build
//! metadata, so a debug build (`0.5.0+dev.<sha>`) is told about a release only
//! when its base version is behind one.
//!
//! # Which page it may point to
//!
//! The release's page is accepted only under [`RELEASE_PAGE_PREFIX`]; any
//! other page makes the whole answer malformed. A later change downloads
//! assets from the same answer, so the place it may point to is pinned here.
//!
//! # Failures
//!
//! A check that fails — the feed's own failures, a tag that is not SemVer, a
//! page outside the prefix — logs one `warn` naming the feed's URL and the
//! cause, and keeps the previous result. It never surfaces to the browser as an error: "not
//! checked yet", "up to date", "turned off" and "every check so far failed"
//! all read as no newer release.

mod check;
pub(crate) use check::with_causes;
mod judge_release;
use judge_release::judge_release;
mod newer_release;
pub use newer_release::NewerRelease;
mod release_check_error;
pub use release_check_error::ReleaseCheckError;

#[cfg(test)]
mod testing;

use std::sync::{Arc, Mutex};

use semver::Version;

use crate::ports::ReleaseFeed;

/// The only place a release's page may be: Delta's own GitHub Releases.
pub const RELEASE_PAGE_PREFIX: &str = "https://github.com/x7c1/delta/releases/";

/// The release check: a feed, this build's version, and the last verdict.
pub struct ReleaseCheck {
    /// `None` when the check is turned off; it then never asks anything.
    feed: Option<Arc<dyn ReleaseFeed>>,
    current: Version,
    newer: Mutex<Option<NewerRelease>>,
}

impl ReleaseCheck {
    /// A check asking `feed` for releases newer than `current_version` (this
    /// build's `CARGO_PKG_VERSION`).
    ///
    /// A `current_version` that is not SemVer turns the check off, with an
    /// `error` logged: there would be nothing to compare against.
    pub fn new(feed: Arc<dyn ReleaseFeed>, current_version: &str) -> Self {
        match Version::parse(current_version) {
            Ok(current) => Self {
                feed: Some(feed),
                current,
                newer: Mutex::new(None),
            },
            Err(err) => {
                tracing::error!(
                    version = current_version,
                    error = %err,
                    "this build's version is not SemVer; the release check is off"
                );
                Self::disabled()
            }
        }
    }

    /// A check that is turned off: it never asks, and never has a newer
    /// release.
    pub fn disabled() -> Self {
        Self {
            feed: None,
            current: Version::new(0, 0, 0),
            newer: Mutex::new(None),
        }
    }

    /// Whether the check asks a feed at all.
    pub fn is_enabled(&self) -> bool {
        self.feed.is_some()
    }

    /// The newer release the last successful check found, if any.
    pub fn newer(&self) -> Option<NewerRelease> {
        self.newer
            .lock()
            .expect("release check mutex poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{check_with, ScriptedFeed};
    use super::*;

    #[tokio::test]
    async fn nothing_is_newer_before_the_first_check() {
        let feed = ScriptedFeed::new(vec![]);
        assert_eq!(check_with(&feed).newer(), None);
        assert_eq!(feed.calls(), 0);
    }

    #[tokio::test]
    async fn a_disabled_check_asks_nothing_and_has_nothing_newer() {
        let check = ReleaseCheck::disabled();
        assert!(!check.is_enabled());
        check.check().await.unwrap();
        assert_eq!(check.newer(), None);
    }

    #[tokio::test]
    async fn a_build_version_that_is_not_semver_turns_the_check_off() {
        let feed = ScriptedFeed::new(vec![]);
        let check = ReleaseCheck::new(Arc::clone(&feed) as Arc<dyn ReleaseFeed>, "unknown");
        assert!(!check.is_enabled());
        check.check().await.unwrap();
        assert_eq!(feed.calls(), 0);
    }
}
