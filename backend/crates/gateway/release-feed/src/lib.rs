//! GitHub-backed [`ReleaseFeed`]: the newest published release of Delta.
//!
//! [`GithubReleaseFeed`] asks GitHub's REST API for the repository's latest
//! release (`GET /repos/x7c1/delta/releases/latest`, which never returns a
//! draft or a pre-release) and reports its `tag_name` and `html_url`
//! unvalidated; the release check in `delta-usecase` judges them.
//!
//! # TLS
//!
//! The client is `reqwest` over rustls with the `ring` provider — no OpenSSL
//! is linked — and verifies certificates through `rustls-platform-verifier`,
//! i.e. against the operating system's trust store (the Keychain on macOS, the
//! system CA store on Linux). Roots bundled into the binary would reject a
//! machine whose network re-signs TLS with a CA the user installed there.
//!
//! # Requests
//!
//! Every request carries a `User-Agent` (GitHub rejects requests without
//! one) and `Accept: application/vnd.github+json`, no token, and gives up
//! after [`REQUEST_TIMEOUT`].
//!
//! [`ReleaseFeed`]: delta_usecase::ReleaseFeed

mod client_build_error;
pub use client_build_error::ClientBuildError;
mod github_release_feed;
pub use github_release_feed::GithubReleaseFeed;

use std::time::Duration;

/// GitHub's endpoint for the newest published release of Delta.
pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/x7c1/delta/releases/latest";

/// How long one request may take, connecting included, before it fails.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
