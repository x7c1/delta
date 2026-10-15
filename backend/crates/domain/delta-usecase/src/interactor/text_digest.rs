//! [`TextDigest`]: what a log line at `info` or above says about a prompt in
//! place of its text.

use std::fmt;

use sha2::{Digest, Sha256};

/// How many hex digits of the SHA-256 a [`TextDigest`] shows.
const HASH_HEX_DIGITS: usize = 8;

/// A prompt's length and a short hash of it, for a log line that compares two
/// texts without recording them.
///
/// Prompts are the user's own words and go to the log only at `debug`. A line
/// at `info` or `warn` that has to say whether two texts differ, and by how
/// much, logs a digest of each instead: equal digests mean equal texts, and the
/// `debug` line logged beside it carries the texts for a session that needs
/// them. The hash is the first [`HASH_HEX_DIGITS`] hex digits of the text's
/// SHA-256, enough to tell texts apart in one log, too few to look one up.
pub(super) struct TextDigest {
    chars: usize,
    hash: String,
}

impl TextDigest {
    /// The digest of `text`.
    pub(super) fn of(text: &str) -> Self {
        let digest = Sha256::digest(text.as_bytes());
        let mut hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        hash.truncate(HASH_HEX_DIGITS);
        Self {
            chars: text.chars().count(),
            hash,
        }
    }
}

impl fmt::Display for TextDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} chars, sha256 {}", self.chars, self.hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_shows_the_length_and_a_short_hash_but_not_the_text() {
        let shown = TextDigest::of("abc").to_string();
        // The SHA-256 of "abc" begins ba7816bf.
        assert_eq!(shown, "3 chars, sha256 ba7816bf");
    }

    #[test]
    fn the_length_counts_characters() {
        assert!(TextDigest::of("日本").to_string().starts_with("2 chars"));
    }
}
