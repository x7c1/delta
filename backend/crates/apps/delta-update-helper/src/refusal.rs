//! Why the helper did not install the update, and the exit status for each.

use std::path::PathBuf;

/// Why the helper did not install the update.
///
/// Each variant exits with a status of its own ([`Self::exit_code`]) and
/// prints its message as one line on stderr, which Delta's server shows the
/// user as the cause. The statuses avoid `126` and `127`, which `pkexec`
/// itself answers with when the authentication was dismissed or refused, and
/// `101`, a Rust panic's.
///
/// The statuses in [`FILE_REJECTED`] say the file itself is not one to
/// install (see [`Self::rejects_file`]); Delta's server reads them so, removes
/// the file and has the user download the update again, rather than offering
/// to install that file from a terminal. [`ALREADY_INSTALLED`] says nothing
/// was installed because the installed app is already the requested version
/// or newer ([`Self::NotNewer`]); the server reads it as installed.
#[derive(Debug, thiserror::Error)]
pub enum Refusal {
    /// The arguments are not `install --version <tag> --file <absolute path>`.
    #[error("usage: delta-update-helper install --version v<version> --file <absolute path>: {0}")]
    Usage(String),
    /// Not running as root, so it could not install anything.
    #[error("the update helper must run as root (through pkexec)")]
    NotRoot,
    /// The file to install is not a regular file it may read: a symlink, a
    /// directory, a device or pipe, too large, or unreadable.
    #[error("refused the update file {}: {reason}", path.display())]
    File { path: PathBuf, reason: String },
    /// The file to install could not be opened or read.
    #[error("refused the update file {}: {source}", path.display())]
    FileUnreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The release's own digest could not be fetched from GitHub: `what`
    /// step failed, for `source` (printed with its own causes).
    #[error("could not read release {tag} from GitHub: {what}: {}", with_causes(source.as_ref()))]
    ReleaseUnavailable {
        tag: String,
        what: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// GitHub answered the request for the release with an error status.
    #[error("could not read release {tag} from GitHub: it answered with HTTP status {status}")]
    ReleaseRefused {
        tag: String,
        status: reqwest::StatusCode,
    },
    /// The release carries no digest for the package's asset.
    #[error("release {tag} states no sha256 digest for {asset}")]
    DigestMissing { tag: String, asset: String },
    /// The release's digest for the asset is not a `sha256:<64 hex digits>`.
    #[error("release {tag} states a digest for {asset} that is not sha256:<hex>: {digest:?}")]
    DigestMalformed {
        tag: String,
        asset: String,
        digest: String,
    },
    /// The file's sha256 is not the one the release states.
    #[error("the update file's sha256 is {actual}, not release {tag}'s {expected}")]
    DigestMismatch {
        tag: String,
        expected: String,
        actual: String,
    },
    /// `dpkg-deb` or `dpkg-query` could not tell the package's or the
    /// installed app's name and version.
    #[error("could not read {what}: {cause}")]
    PackageUnreadable { what: &'static str, cause: String },
    /// The installed app's version, as `dpkg-query` printed it, is not a
    /// version.
    #[error("could not read the installed delta-desktop's version: {installed:?} is not a version: {source}")]
    InstalledVersionMalformed {
        installed: String,
        #[source]
        source: semver::Error,
    },
    /// `dpkg-deb`, `dpkg-query` or `apt-get` could not be started.
    #[error("could not run {program}: {source}")]
    Unrunnable {
        program: &'static str,
        #[source]
        source: std::io::Error,
    },
    /// The package is not Delta's desktop app.
    #[error("the update file is the package {0:?}, not delta-desktop")]
    NotDelta(String),
    /// The package's version is not the one asked for.
    #[error("the update file is version {actual}, not the requested {requested}")]
    VersionMismatch { requested: String, actual: String },
    /// The package is not newer than the installed app: the requested
    /// version, or a newer one, is installed already (from a terminal, say).
    /// Not a rejection of the file, which matched the release; it exits with
    /// [`ALREADY_INSTALLED`].
    #[error("the update file's version {package} is not newer than the installed {installed}")]
    NotNewer { package: String, installed: String },
    /// `apt-get install` failed.
    #[error("apt-get could not install the update: {0}")]
    InstallFailed(String),
    /// The helper's own working files could not be set up.
    #[error("{what}: {source}")]
    Io {
        what: String,
        #[source]
        source: std::io::Error,
    },
}

/// The exit statuses of the refusals that reject the file itself
/// ([`Refusal::rejects_file`]). Delta's server holds the same range
/// (`HELPER_REJECTED_FILE` in the `update-installer` crate).
pub const FILE_REJECTED: std::ops::RangeInclusive<u8> = 10..=19;

/// The exit status of [`Refusal::NotNewer`]: the installed app is already the
/// requested version or newer, so there was nothing to install. Delta's
/// server holds the same status (`HELPER_ALREADY_INSTALLED` in the
/// `update-installer` crate) and reads it as installed.
pub const ALREADY_INSTALLED: u8 = 30;

impl Refusal {
    /// The status the helper exits with: one per variant, never `0`, `126`,
    /// `127` or `101`, and in [`FILE_REJECTED`] exactly when the refusal
    /// [rejects the file](Self::rejects_file).
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::NotRoot => 3,
            Self::File { .. } => 10,
            Self::DigestMissing { .. } => 12,
            Self::DigestMalformed { .. } => 13,
            Self::DigestMismatch { .. } => 14,
            Self::NotDelta(_) => 16,
            Self::VersionMismatch { .. } => 17,
            Self::FileUnreadable { .. } => 19,
            Self::InstallFailed(_) => 20,
            Self::Io { .. } => 21,
            Self::ReleaseRefused { .. } => 22,
            Self::InstalledVersionMalformed { .. } => 23,
            Self::Unrunnable { .. } => 24,
            Self::ReleaseUnavailable { .. } => 25,
            Self::PackageUnreadable { .. } => 26,
            Self::NotNewer { .. } => ALREADY_INSTALLED,
        }
    }

    /// Whether the refusal is of the file itself: not a regular file the
    /// helper may read, not matching the digest the release states (or the
    /// release stating none it can use), or not `delta-desktop` at the
    /// version asked for. Such a file is not to be installed by any means,
    /// from a terminal included. Every other refusal is about this machine or
    /// the network (GitHub out of reach, `dpkg` or `apt-get` failing), and
    /// leaves installing the file from a terminal open, except
    /// [`Self::NotNewer`], which says the update is installed already.
    pub fn rejects_file(&self) -> bool {
        match self {
            Self::File { .. }
            | Self::FileUnreadable { .. }
            | Self::DigestMissing { .. }
            | Self::DigestMalformed { .. }
            | Self::DigestMismatch { .. }
            | Self::NotDelta(_)
            | Self::VersionMismatch { .. } => true,
            Self::Usage(_)
            | Self::NotRoot
            | Self::ReleaseUnavailable { .. }
            | Self::ReleaseRefused { .. }
            | Self::PackageUnreadable { .. }
            | Self::InstalledVersionMalformed { .. }
            | Self::Unrunnable { .. }
            | Self::InstallFailed(_)
            | Self::Io { .. }
            | Self::NotNewer { .. } => false,
        }
    }
}

/// `err` and its sources, joined with `: `, so the one line the helper
/// prints says why all the way down (`reqwest`'s own message, for one,
/// leaves out the cause of a failed connection).
fn with_causes(err: &(dyn std::error::Error + 'static)) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_has_a_status_of_its_own_clear_of_pkexecs_and_ranged_by_kind() {
        let io = || std::io::Error::other("x");
        let all = [
            Refusal::Usage(String::new()),
            Refusal::NotRoot,
            Refusal::File {
                path: PathBuf::new(),
                reason: String::new(),
            },
            Refusal::FileUnreadable {
                path: PathBuf::new(),
                source: io(),
            },
            Refusal::ReleaseUnavailable {
                tag: String::new(),
                what: "",
                source: Box::new(io()),
            },
            Refusal::ReleaseRefused {
                tag: String::new(),
                status: reqwest::StatusCode::NOT_FOUND,
            },
            Refusal::DigestMissing {
                tag: String::new(),
                asset: String::new(),
            },
            Refusal::DigestMalformed {
                tag: String::new(),
                asset: String::new(),
                digest: String::new(),
            },
            Refusal::DigestMismatch {
                tag: String::new(),
                expected: String::new(),
                actual: String::new(),
            },
            Refusal::PackageUnreadable {
                what: "",
                cause: String::new(),
            },
            Refusal::InstalledVersionMalformed {
                installed: String::new(),
                source: semver::Version::parse("").unwrap_err(),
            },
            Refusal::Unrunnable {
                program: "",
                source: io(),
            },
            Refusal::NotDelta(String::new()),
            Refusal::VersionMismatch {
                requested: String::new(),
                actual: String::new(),
            },
            Refusal::NotNewer {
                package: String::new(),
                installed: String::new(),
            },
            Refusal::InstallFailed(String::new()),
            Refusal::Io {
                what: String::new(),
                source: io(),
            },
        ];
        for refusal in &all {
            assert_eq!(
                FILE_REJECTED.contains(&refusal.exit_code()),
                refusal.rejects_file(),
                "{refusal:?}"
            );
            assert_eq!(
                refusal.exit_code() == ALREADY_INSTALLED,
                matches!(refusal, Refusal::NotNewer { .. }),
                "{refusal:?}"
            );
        }
        let mut codes: Vec<u8> = all.iter().map(Refusal::exit_code).collect();
        for code in &codes {
            assert!(![0, 101, 126, 127].contains(code), "{code}");
        }
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), all.len());
    }
}
