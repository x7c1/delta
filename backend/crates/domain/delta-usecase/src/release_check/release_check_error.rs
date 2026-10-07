use super::RELEASE_PAGE_PREFIX;
use crate::ports::ReleaseFeedError;

/// Why a check produced no verdict.
#[derive(Debug, thiserror::Error)]
pub enum ReleaseCheckError {
    /// The feed could not report a release.
    #[error(transparent)]
    Feed(#[from] ReleaseFeedError),
    /// The release's tag is not `v<semver>`. `cause` is SemVer's complaint
    /// when the tag has its `v` but the rest does not parse.
    #[error("the release tag {tag:?} is not v<semver>")]
    Tag {
        tag: String,
        #[source]
        cause: Option<semver::Error>,
    },
    /// The release's page is outside [`RELEASE_PAGE_PREFIX`].
    #[error("the release page {0:?} is outside {RELEASE_PAGE_PREFIX}")]
    Page(String),
}
