//! The newest published release as the feed reported it.

use super::ReleaseAsset;

/// The newest published release as the feed reported it, before any
/// validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedRelease {
    /// The release's git tag, expected to be `v<semver>`.
    pub tag_name: String,
    /// The release's web page.
    pub html_url: String,
    /// The files attached to the release, in the order the feed listed them.
    pub assets: Vec<ReleaseAsset>,
}
