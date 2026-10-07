use super::RELEASE_DOWNLOAD_PREFIX;

/// Why an update download was refused before anything was fetched.
#[derive(Debug, Clone, thiserror::Error)]
pub enum UpdateRefusal {
    /// The CLI launched the server: only the desktop app can be replaced.
    #[error("updating is offered only in the desktop app")]
    CliLauncher,
    /// A desktop app built locally: replacing it with the release would roll
    /// its tree back.
    #[error("this desktop app was built locally; update it by rebuilding it (make desktop)")]
    LocalBuild,
    /// The app could not set up its HTTPS client.
    #[error("this app cannot download updates: its HTTPS client could not be set up")]
    NoDownloader,
    /// No newer release is known: not checked yet, up to date, the check
    /// turned off, or every check so far failed.
    #[error("no newer release is known")]
    NoNewerRelease,
    /// No release carries an asset for this platform.
    #[error("no release asset is published for {platform}")]
    UnsupportedPlatform { platform: String },
    /// The newer release lacks this platform's asset.
    #[error("release {version} has no asset {asset}")]
    MissingAsset { version: String, asset: String },
    /// This platform's asset states no sha256 digest, so it could not be
    /// verified and is never downloaded.
    #[error("release {version}'s asset {asset} states no sha256 digest to verify it against")]
    NoDigest { version: String, asset: String },
    /// The asset's download URL is outside [`RELEASE_DOWNLOAD_PREFIX`].
    #[error("the asset's download URL {0:?} is outside {RELEASE_DOWNLOAD_PREFIX}")]
    UntrustedUrl(String),
}
