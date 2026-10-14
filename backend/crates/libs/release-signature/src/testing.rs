//! A bundle and its signatures made with a key pair generated for the tests
//! (with `minisign -G -W`; its secret half was discarded), for the tests of
//! the crates that check signatures. Never the release key.
//!
//! Every signature here is of [`BUNDLE`]; they differ in the key they were
//! made with and the trusted comment they carry.

use semver::Version;

use crate::ReleaseKey;

/// The public half of the test key pair, in minisign's base64 form.
pub const TEST_PUBLIC_KEY: &str = "RWSkN6a5nKrwRME7A3D56twI4KzWuT/SdOzku7bXP+0igA3QQP0NjbsT";

/// The name the signatures here sign [`BUNDLE`] as.
pub const BUNDLE_NAME: &str = "delta-desktop_0.6.0_amd64.deb";

/// The contents every signature here signs.
pub const BUNDLE: &[u8] = include_bytes!("testing/bundle");

/// [`BUNDLE`] signed with the test key as [`BUNDLE_NAME`] of
/// [`bundle_version`], as the release workflow signs a bundle.
pub const SIGNED: &str = include_str!("testing/signed.minisig");

/// [`BUNDLE`] signed with the test key as `Delta_0.6.0_aarch64.dmg`.
pub const OTHER_FILE: &str = include_str!("testing/other-file.minisig");

/// [`BUNDLE`] signed with the test key as [`BUNDLE_NAME`] of version 0.5.9.
pub const OTHER_VERSION: &str = include_str!("testing/other-version.minisig");

/// [`BUNDLE`] signed with the test key under minisign's own kind of
/// trusted comment, which names no file and version.
pub const BARE_COMMENT: &str = include_str!("testing/bare-comment.minisig");

/// [`BUNDLE`] signed with the test key in minisign's legacy format, which
/// signs the file rather than its hash.
pub const LEGACY: &str = include_str!("testing/legacy.minisig");

/// [`BUNDLE`] signed with another key as [`BUNDLE_NAME`] of
/// [`bundle_version`].
pub const OTHER_KEY: &str = include_str!("testing/other-key.minisig");

/// The test key, to check the signatures here against.
pub fn test_key() -> ReleaseKey {
    ReleaseKey::from_base64(TEST_PUBLIC_KEY).expect("the test key is a minisign public key")
}

/// The version [`SIGNED`] signs [`BUNDLE`] as: 0.6.0.
pub fn bundle_version() -> Version {
    Version::new(0, 6, 0)
}
