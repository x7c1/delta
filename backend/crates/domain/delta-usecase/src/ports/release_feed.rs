//! Where the newest published release of Delta comes from.
//!
//! The [`ReleaseFeed`] port asks the outside world (GitHub's releases API, in
//! production) for the newest published release and hands back what it said,
//! unvalidated. Judging the answer — whether its tag is SemVer, whether its
//! page is one Delta trusts, whether it is newer than this build — is the
//! release check's job ([`crate::ReleaseCheck`]), so a test can substitute a
//! feed that answers anything at all.

use async_trait::async_trait;

/// The newest published release as the feed reported it, before any
/// validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedRelease {
    /// The release's git tag, expected to be `v<semver>`.
    pub tag_name: String,
    /// The release's web page.
    pub html_url: String,
}

/// A failure from below the port (an HTTP client's, a JSON parser's), kept
/// whole so its cause chain still reaches the log.
pub type FeedCause = Box<dyn std::error::Error + Send + Sync>;

/// Why the feed could not report a release.
///
/// `Display` names only the kind of failure; the underlying failure is the
/// error's `source`, so a log should render the whole chain.
#[derive(Debug, thiserror::Error)]
pub enum ReleaseFeedError {
    /// No answer arrived: offline, DNS, TLS, a refused connection, a timeout.
    #[error("no answer arrived from the feed")]
    Request(#[source] FeedCause),
    /// The feed answered with a non-2xx status (GitHub's rate limit is a 403).
    #[error("the feed answered with HTTP status {0}")]
    Status(u16),
    /// The feed answered 2xx with a body that is not a release.
    #[error("the answer is not a release")]
    Malformed(#[source] FeedCause),
}

/// Reports the newest published release.
#[async_trait]
pub trait ReleaseFeed: Send + Sync {
    /// Ask for the newest published release (never a draft or pre-release).
    async fn latest_release(&self) -> Result<PublishedRelease, ReleaseFeedError>;

    /// Where the feed asks, named in the log when a check fails: a failure's
    /// own message does not always say (an HTTP status does not).
    fn url(&self) -> &str;
}
