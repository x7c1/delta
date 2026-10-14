//! Why a bundle's signature was refused.

/// Why a bundle's signature was refused. Every variant means the bundle is
/// not to be installed: there is no fallback to the digest alone.
#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    /// The release carries no signature for the bundle, or the signature
    /// file is gone.
    #[error("the release carries no signature for {bundle}")]
    Missing { bundle: String },
    /// The signature file is larger than any minisign signature
    /// ([`crate::MAX_SIGNATURE_BYTES`]).
    #[error("the signature is larger than {limit} bytes, so it is not a minisign signature")]
    TooLarge { limit: usize },
    /// The signature file is not a minisign signature.
    #[error("the signature is not a minisign signature")]
    Malformed {
        #[source]
        source: minisign_verify::Error,
    },
    /// A legacy (not pre-hashed) signature, or one of an algorithm this
    /// check does not accept.
    #[error("the signature is not a pre-hashed Ed25519 minisign signature")]
    Unsupported {
        #[source]
        source: minisign_verify::Error,
    },
    /// The signature was made with another key than the release key.
    #[error("the signature was made with another key than Delta's release key")]
    OtherKey,
    /// The signature's trusted comment is not `file:<name> version:<version>`.
    #[error("the signature's trusted comment {signed:?} names no file and version")]
    CommentMalformed { signed: String },
    /// The signature is for another file.
    #[error("the signature is for {signed}, not {expected}")]
    OtherFile { expected: String, signed: String },
    /// The signature is for another version of the file.
    #[error("the signature is for version {signed}, not {expected}")]
    OtherVersion { expected: String, signed: String },
    /// The bundle, or the trusted comment, does not verify against the
    /// signature: the file is not the one the release workflow signed.
    #[error("the file does not match its signature")]
    Invalid,
    /// The bundle could not be read while it was checked.
    #[error("could not read the file to check its signature")]
    Unreadable {
        #[source]
        source: std::io::Error,
    },
    /// The key the signature is checked against is not a minisign public
    /// key.
    #[error("the key to check the signature with is not a minisign public key")]
    Key {
        #[source]
        source: minisign_verify::Error,
    },
}
