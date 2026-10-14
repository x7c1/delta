//! The public key a bundle's signature is checked against.

use minisign_verify::PublicKey;

use crate::{SignatureError, PUBLIC_KEY};

/// The public key a bundle's signature is checked against.
#[derive(Debug, Clone)]
pub struct ReleaseKey(PublicKey);

impl ReleaseKey {
    /// The release workflow's key, [`PUBLIC_KEY`]: the only key the app and
    /// the update helper accept.
    pub fn production() -> Self {
        Self::from_base64(PUBLIC_KEY).expect("the compiled-in release key is a minisign public key")
    }

    /// A key in minisign's base64 form (the second line of a `.pub` file).
    ///
    /// For tests, which sign with a key pair of their own (the `testing`
    /// module, behind the `testing` feature); the app and the helper use
    /// [`Self::production`].
    pub fn from_base64(key: &str) -> Result<Self, SignatureError> {
        PublicKey::from_base64(key)
            .map(Self)
            .map_err(|source| SignatureError::Key { source })
    }

    pub(crate) fn key(&self) -> &PublicKey {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compiled_in_key_is_a_minisign_public_key() {
        ReleaseKey::production();
    }
}
