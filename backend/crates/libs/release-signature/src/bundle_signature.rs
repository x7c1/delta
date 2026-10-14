//! A bundle's signature, and checking the bundle's contents against it.

use std::io::{ErrorKind, Read};

use minisign_verify::{Error as MinisignError, Signature, StreamVerifier};
use semver::Version;

use crate::{trusted_comment, ReleaseKey, SignatureError, MAX_SIGNATURE_BYTES};

/// The signature of one bundle, already found to be made with the release
/// key and to name the bundle and version expected; what is left is
/// checking the bundle's contents against it ([`Self::check`],
/// [`Self::verify_reader`]).
pub struct BundleSignature {
    key: ReleaseKey,
    signature: Signature,
}

impl BundleSignature {
    /// Read the signature file's contents `text` as the signature of the
    /// bundle named `bundle` of `version`, made with `key`.
    ///
    /// Refused, before the bundle is read, when `text` is larger than
    /// [`MAX_SIGNATURE_BYTES`] or not a minisign signature, when it is a
    /// legacy signature, when it was made with another key, and when its
    /// trusted comment is not exactly [`trusted_comment`] of `bundle` and
    /// `version`. The trusted comment itself is only proven to be the
    /// signer's once the bundle has been checked: a forged one fails then.
    pub fn parse(
        key: &ReleaseKey,
        text: &[u8],
        bundle: &str,
        version: &Version,
    ) -> Result<Self, SignatureError> {
        if text.len() > MAX_SIGNATURE_BYTES {
            return Err(SignatureError::TooLarge {
                limit: MAX_SIGNATURE_BYTES,
            });
        }
        let malformed = |source| SignatureError::Malformed { source };
        let text =
            std::str::from_utf8(text).map_err(|_| malformed(MinisignError::InvalidEncoding))?;
        let signature = Signature::decode(text).map_err(|err| match err {
            MinisignError::UnsupportedAlgorithm => SignatureError::Unsupported { source: err },
            other => malformed(other),
        })?;
        let parsed = Self {
            key: key.clone(),
            signature,
        };
        // Checks the key id and that the signature is pre-hashed.
        parsed.check()?;
        check_comment(parsed.signature.trusted_comment(), bundle, version)?;
        Ok(parsed)
    }

    /// A check of the bundle's contents, fed as they stream in.
    pub fn check(&self) -> Result<BundleCheck<'_>, SignatureError> {
        self.key
            .key()
            .verify_stream(&self.signature)
            .map(BundleCheck)
            .map_err(|err| match err {
                MinisignError::UnexpectedKeyId => SignatureError::OtherKey,
                other => SignatureError::Unsupported { source: other },
            })
    }

    /// Check the bundle `reader` reads, to its end, against the signature.
    pub fn verify_reader(&self, mut reader: impl Read) -> Result<(), SignatureError> {
        let mut check = self.check()?;
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(source) => return Err(SignatureError::Unreadable { source }),
            };
            check.update(&buffer[..read]);
        }
        check.finish()
    }
}

/// The check of one bundle's contents against its signature, fed in chunks.
pub struct BundleCheck<'a>(StreamVerifier<'a>);

impl BundleCheck<'_> {
    /// Feed the next chunk of the bundle.
    pub fn update(&mut self, chunk: &[u8]) {
        self.0.update(chunk);
    }

    /// Whether everything fed, and the signature's trusted comment, verify
    /// against the signature.
    pub fn finish(mut self) -> Result<(), SignatureError> {
        self.0.finalize().map_err(|err| match err {
            MinisignError::InvalidSignature => SignatureError::Invalid,
            other => SignatureError::Unsupported { source: other },
        })
    }
}

/// Accept `signed`, a signature's trusted comment, only when it is exactly
/// [`trusted_comment`] of `bundle` and `version`.
fn check_comment(signed: &str, bundle: &str, version: &Version) -> Result<(), SignatureError> {
    if signed == trusted_comment(bundle, version) {
        return Ok(());
    }
    let malformed = || SignatureError::CommentMalformed {
        signed: signed.to_owned(),
    };
    let (file, signed_version) = signed
        .strip_prefix("file:")
        .and_then(|rest| rest.split_once(" version:"))
        .ok_or_else(malformed)?;
    if file != bundle {
        return Err(SignatureError::OtherFile {
            expected: bundle.to_owned(),
            signed: file.to_owned(),
        });
    }
    let expected = version.to_string();
    if signed_version != expected {
        return Err(SignatureError::OtherVersion {
            expected,
            signed: signed_version.to_owned(),
        });
    }
    Err(malformed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        bundle_version, test_key, BARE_COMMENT, BUNDLE, BUNDLE_NAME, LEGACY, OTHER_FILE, OTHER_KEY,
        OTHER_VERSION, SIGNED,
    };

    fn verify(signature: &str, bundle: &[u8]) -> Result<(), SignatureError> {
        BundleSignature::parse(
            &test_key(),
            signature.as_bytes(),
            BUNDLE_NAME,
            &bundle_version(),
        )?
        .verify_reader(bundle)
    }

    #[test]
    fn a_bundle_signed_with_the_key_for_its_name_and_version_verifies() {
        verify(SIGNED, BUNDLE).unwrap();
    }

    #[test]
    fn a_bundle_fed_in_chunks_verifies_as_a_whole() {
        let signature = BundleSignature::parse(
            &test_key(),
            SIGNED.as_bytes(),
            BUNDLE_NAME,
            &bundle_version(),
        )
        .unwrap();
        let mut check = signature.check().unwrap();
        for chunk in BUNDLE.chunks(5) {
            check.update(chunk);
        }
        check.finish().unwrap();
    }

    #[test]
    fn a_modified_bundle_is_refused() {
        let mut modified = BUNDLE.to_vec();
        modified[0] ^= 1;
        for bundle in [&modified[..], &BUNDLE[..BUNDLE.len() - 1], b""] {
            let err = verify(SIGNED, bundle).unwrap_err();
            assert!(matches!(err, SignatureError::Invalid), "{err:?}");
        }
    }

    #[test]
    fn a_signature_made_with_another_key_is_refused() {
        let err = verify(OTHER_KEY, BUNDLE).unwrap_err();
        assert!(matches!(err, SignatureError::OtherKey), "{err:?}");
    }

    #[test]
    fn the_release_key_refuses_a_signature_made_with_the_test_key() {
        let err = BundleSignature::parse(
            &ReleaseKey::production(),
            SIGNED.as_bytes(),
            BUNDLE_NAME,
            &bundle_version(),
        )
        .err()
        .unwrap();
        assert!(matches!(err, SignatureError::OtherKey), "{err:?}");
    }

    #[test]
    fn a_signature_naming_another_file_is_refused() {
        let err = verify(OTHER_FILE, BUNDLE).unwrap_err();
        assert!(
            matches!(
                &err,
                SignatureError::OtherFile { expected, signed }
                    if expected == BUNDLE_NAME && signed == "Delta_0.6.0_aarch64.dmg"
            ),
            "{err:?}"
        );
    }

    #[test]
    fn a_signature_naming_another_version_is_refused() {
        let err = verify(OTHER_VERSION, BUNDLE).unwrap_err();
        assert!(
            matches!(
                &err,
                SignatureError::OtherVersion { expected, signed }
                    if expected == "0.6.0" && signed == "0.5.9"
            ),
            "{err:?}"
        );
    }

    #[test]
    fn a_signature_naming_no_file_and_version_is_refused() {
        let err = verify(BARE_COMMENT, BUNDLE).unwrap_err();
        assert!(
            matches!(&err, SignatureError::CommentMalformed { signed } if signed == "timestamp:1760000000"),
            "{err:?}"
        );
    }

    #[test]
    fn a_trusted_comment_altered_after_signing_is_refused() {
        // The comment the signer wrote names another version; rewritten to
        // name this one, it passes the name check and fails verification.
        let altered = OTHER_VERSION.replace("version:0.5.9", "version:0.6.0");
        let err = verify(&altered, BUNDLE).unwrap_err();
        assert!(matches!(err, SignatureError::Invalid), "{err:?}");
    }

    #[test]
    fn a_legacy_signature_is_refused() {
        let err = verify(LEGACY, BUNDLE).unwrap_err();
        assert!(matches!(err, SignatureError::Unsupported { .. }), "{err:?}");
    }

    #[test]
    fn anything_but_a_signature_is_refused() {
        for text in ["", "not a signature", "untrusted comment: x\n!!!\n"] {
            let err = verify(text, BUNDLE).unwrap_err();
            assert!(
                matches!(err, SignatureError::Malformed { .. }),
                "{text:?}: {err:?}"
            );
        }
        let err = BundleSignature::parse(&test_key(), b"\xff\xfe", BUNDLE_NAME, &bundle_version())
            .err()
            .unwrap();
        assert!(matches!(err, SignatureError::Malformed { .. }), "{err:?}");
    }

    #[test]
    fn a_signature_file_larger_than_any_signature_is_refused_unread() {
        let large = format!("{SIGNED}{}", " ".repeat(MAX_SIGNATURE_BYTES));
        let err = verify(&large, BUNDLE).unwrap_err();
        assert!(
            matches!(err, SignatureError::TooLarge { limit } if limit == MAX_SIGNATURE_BYTES),
            "{err:?}"
        );
    }

    #[test]
    fn a_read_error_is_reported_as_one() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("the disk is gone"))
            }
        }
        let signature = BundleSignature::parse(
            &test_key(),
            SIGNED.as_bytes(),
            BUNDLE_NAME,
            &bundle_version(),
        )
        .unwrap();
        let err = signature.verify_reader(Failing).unwrap_err();
        assert!(matches!(err, SignatureError::Unreadable { .. }), "{err:?}");
    }
}
