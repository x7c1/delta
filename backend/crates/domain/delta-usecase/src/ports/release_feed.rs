//! Where the newest published release of Delta comes from.
//!
//! The [`ReleaseFeed`] port asks the outside world (GitHub's releases API, in
//! production) for the newest published release and hands back what it said,
//! unvalidated. Judging the answer — whether its tag is SemVer, whether its
//! page is one Delta trusts, whether it is newer than this build — is the
//! release check's job ([`crate::ReleaseCheck`]), so a test can substitute a
//! feed that answers anything at all.

use async_trait::async_trait;

use super::{PublishedRelease, ReleaseFeedError};

/// A failure from below the port (an HTTP client's, a JSON parser's), kept
/// whole so its cause chain still reaches the log.
pub type FeedCause = Box<dyn std::error::Error + Send + Sync>;

/// Reports the newest published release.
#[async_trait]
pub trait ReleaseFeed: Send + Sync {
    /// Ask for the newest published release (never a draft or pre-release).
    async fn latest_release(&self) -> Result<PublishedRelease, ReleaseFeedError>;

    /// Where the feed asks, named in the log when a check fails: a failure's
    /// own message does not always say (an HTTP status does not).
    fn url(&self) -> &str;
}
