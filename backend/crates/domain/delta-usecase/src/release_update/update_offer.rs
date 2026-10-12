//! What the browser may offer next to the newer-release notice.

/// What the browser may offer next to the newer-release notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOffer {
    /// The Update action: download (and later apply) the newer release, which
    /// has an asset this app would download.
    Update,
    /// A hint that this desktop app was built locally and is updated by
    /// rebuilding it (`make desktop`).
    Rebuild,
    /// Nothing beyond the notice and its link: the CLI server, a desktop
    /// release build that cannot download ([`NotOffered::NoDownloader`]), or
    /// a newer release with no asset this platform may download.
    ///
    /// [`NotOffered::NoDownloader`]: super::NotOffered::NoDownloader
    None,
}
