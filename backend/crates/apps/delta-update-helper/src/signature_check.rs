//! Checking the staged copy of the update against the release's signature.

use std::fs::File;
use std::path::Path;

use release_signature::{BundleSignature, ReleaseKey};
use semver::Version;

use crate::Refusal;

/// Check the file at `path` (the helper's own staged copy) against
/// `signature`, the release's signature of its asset `asset` of `version`,
/// made with `key`.
///
/// The helper's own check: whatever the caller checked before it, the file
/// is installed only when the signature was made with `key`, names exactly
/// `asset` and `version`, and verifies the file's contents.
pub fn check_signature(
    key: &ReleaseKey,
    path: &Path,
    signature: &[u8],
    asset: &str,
    version: &Version,
) -> Result<(), Refusal> {
    let rejected = |source| Refusal::SignatureRejected {
        asset: asset.to_owned(),
        source,
    };
    let parsed = BundleSignature::parse(key, signature, asset, version).map_err(rejected)?;
    let file = File::open(path).map_err(|source| Refusal::Io {
        what: format!("could not open the staged copy {}", path.display()),
        source,
    })?;
    parsed.verify_reader(file).map_err(rejected)
}

#[cfg(test)]
mod tests {
    use release_signature::testing::{
        bundle_version, test_key, BUNDLE, BUNDLE_NAME, OTHER_FILE, OTHER_KEY, OTHER_VERSION, SIGNED,
    };
    use release_signature::SignatureError;

    use super::*;

    /// Check `contents`, written to a file, against `signature`.
    fn check(contents: &[u8], signature: &str) -> Result<(), Refusal> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.deb");
        std::fs::write(&path, contents).unwrap();
        check_signature(
            &test_key(),
            &path,
            signature.as_bytes(),
            BUNDLE_NAME,
            &bundle_version(),
        )
    }

    fn refused_for(result: Result<(), Refusal>) -> SignatureError {
        match result {
            Err(Refusal::SignatureRejected { asset, source }) => {
                assert_eq!(asset, BUNDLE_NAME);
                source
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_file_signed_for_its_name_and_version_passes() {
        check(BUNDLE, SIGNED).unwrap();
    }

    /// The caller has no say: there is no argument to tell the helper the
    /// file was checked (see `install_request`), and a file the signature
    /// does not verify is refused whatever it matched before.
    #[test]
    fn a_file_the_signature_does_not_verify_is_refused() {
        let mut modified = BUNDLE.to_vec();
        modified[3] ^= 0x20;
        assert!(matches!(
            refused_for(check(&modified, SIGNED)),
            SignatureError::Invalid
        ));
    }

    #[test]
    fn a_signature_of_another_key_file_or_version_is_refused() {
        assert!(matches!(
            refused_for(check(BUNDLE, OTHER_KEY)),
            SignatureError::OtherKey
        ));
        assert!(matches!(
            refused_for(check(BUNDLE, OTHER_FILE)),
            SignatureError::OtherFile { .. }
        ));
        assert!(matches!(
            refused_for(check(BUNDLE, OTHER_VERSION)),
            SignatureError::OtherVersion { .. }
        ));
        assert!(matches!(
            refused_for(check(BUNDLE, "")),
            SignatureError::Malformed { .. }
        ));
    }

    #[test]
    fn the_release_key_refuses_the_test_signature() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.deb");
        std::fs::write(&path, BUNDLE).unwrap();
        let result = check_signature(
            &ReleaseKey::production(),
            &path,
            SIGNED.as_bytes(),
            BUNDLE_NAME,
            &bundle_version(),
        );
        assert!(matches!(refused_for(result), SignatureError::OtherKey));
    }

    #[test]
    fn every_signature_refusal_rejects_the_file() {
        let refusal = check(BUNDLE, OTHER_KEY).unwrap_err();
        assert!(refusal.rejects_file(), "{refusal:?}");
    }
}
