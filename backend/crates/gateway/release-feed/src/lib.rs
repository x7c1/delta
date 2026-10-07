//! GitHub-backed [`ReleaseFeed`] and [`AssetDownloader`]: the newest
//! published release of Delta, and its files.
//!
//! [`GithubReleaseFeed`] asks GitHub's REST API for the repository's latest
//! release (`GET /repos/x7c1/delta/releases/latest`, which never returns a
//! draft or a pre-release) and reports its `tag_name`, `html_url` and assets
//! (name, `browser_download_url`, `digest`) unvalidated; the release check in
//! `delta-usecase` judges them.
//!
//! [`GithubAssetDownloader`] downloads one asset into a directory, computing
//! its sha256 while it streams in and comparing it with the asset's
//! `sha256:<hex>` digest; which asset, and whether its URL is trusted, is the
//! release update's decision in `delta-usecase`.
//!
//! [`remove_stale_updates`] clears the downloads the running app has caught
//! up with out of that directory, at startup.
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
//! one) and no token. The feed's request also sends
//! `Accept: application/vnd.github+json` and gives up after
//! [`REQUEST_TIMEOUT`]. A download of several MB has no cap on the whole
//! transfer: it gives up when connecting takes longer than
//! [`DOWNLOAD_CONNECT_TIMEOUT`] or no data arrives for
//! [`DOWNLOAD_STALL_TIMEOUT`].
//!
//! [`ReleaseFeed`]: delta_usecase::ReleaseFeed
//! [`AssetDownloader`]: delta_usecase::AssetDownloader

mod client_build_error;
pub use client_build_error::ClientBuildError;
mod github_asset_downloader;
pub use github_asset_downloader::GithubAssetDownloader;
mod github_release_feed;
pub use github_release_feed::GithubReleaseFeed;
mod https_client;
mod stale_updates;
pub use stale_updates::remove_stale_updates;
#[cfg(test)]
mod test_server;

use std::time::Duration;

/// GitHub's endpoint for the newest published release of Delta.
pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/x7c1/delta/releases/latest";

/// How long one feed request may take, connecting included, before it fails.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a download may take to connect before it fails.
pub const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a download may go without receiving data before it fails.
pub const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(30);
