//! The key Delta's release bundles are signed with, and the check of a bundle
//! against its signature.
//!
//! The release workflow signs every bundle it publishes with
//! [minisign](https://jedisct1.github.io/minisign/) (Ed25519), in a job of
//! its own that holds the secret key and runs no project code, and attaches
//! the signature next to the bundle as `<bundle>.minisig`
//! ([`signature_name`]). GitHub's sha256 digest of an asset proves only that
//! the download is what was uploaded; the signature proves the release
//! pipeline produced it. The desktop app checks a downloaded update against
//! the signature before it offers it for install, and the Linux root helper
//! checks it again on its own before installing it. Both use
//! [`ReleaseKey::production`], the one place [`PUBLIC_KEY`] is read.
//!
//! A signature is accepted only when all of these hold:
//!
//! - it was made with the release key (its key id is the key's);
//! - it is a pre-hashed signature (minisign's default since 0.8), so a large
//!   bundle is checked as it streams rather than held in memory;
//! - its trusted comment names exactly the bundle's file name and the
//!   version being installed ([`trusted_comment`]), so a valid signature of
//!   another file, or of an older release, is refused;
//! - the bundle's contents and the trusted comment verify against it.
//!
//! Each failure is a [`SignatureError`] of its own.

mod bundle_signature;
pub use bundle_signature::{BundleCheck, BundleSignature};
mod release_key;
pub use release_key::ReleaseKey;
mod signature_error;
pub use signature_error::SignatureError;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

use semver::Version;

/// The public half of the key the release workflow signs bundles with
/// (minisign key id `C2B89994B1AA8DAF`), in minisign's base64 form.
///
/// Compiled into the desktop app and into the Linux update helper through
/// [`ReleaseKey::production`]; never read from a file or the environment.
/// Rotating it is a release of its own (docs/guides/release.md).
pub const PUBLIC_KEY: &str = "RWSvjaqxlJm4wr9SiKBHDAlbhBASzeSf0dbg9loPJdgbTFQOEOX5J1Pl";

/// The most a signature file may hold. A minisign signature with the
/// release workflow's short trusted comment is about 300 bytes; anything
/// larger is not one, and is not read further.
pub const MAX_SIGNATURE_BYTES: usize = 4096;

/// The name of the signature file of the bundle named `bundle`, as the
/// release workflow attaches it: `<bundle>.minisig`.
pub fn signature_name(bundle: &str) -> String {
    format!("{bundle}.minisig")
}

/// The trusted comment the release workflow signs the bundle named `bundle`
/// of `version` with: `file:<bundle> version:<version>`.
pub fn trusted_comment(bundle: &str, version: &Version) -> String {
    format!("file:{bundle} version:{version}")
}
