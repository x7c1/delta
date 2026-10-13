//! Why the release feed could not report a release.

use super::FeedCause;

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
