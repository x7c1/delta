//! URL-safe path encoding for the repository-scan-root DELETE endpoint.
//!
//! `DELETE /api/repository-scan-roots/:path_b64` receives the registered
//! absolute path as a URL-safe base64 token in the path segment, so the
//! embedded `/` characters survive the route match without `%2F` escaping
//! quirks.

#[cfg(test)]
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encode `path` as URL-safe base64 (RFC 4648 §5), no padding.
///
/// Only used by tests in this crate (production callers reach the encoded form
/// through the frontend), but kept `pub(crate)` so the test helpers stay
/// symmetric with [`decode`].
#[cfg(test)]
pub(crate) fn encode(path: &str) -> String {
    base64url_encode(path.as_bytes())
}

/// Decode a URL-safe base64 token back into the original path string. Returns
/// `None` when the token is not valid base64url or its bytes are not valid
/// UTF-8 (an absolute path always is, so the latter signals a corrupted
/// token, not an unsupported encoding).
pub(crate) fn decode(token: &str) -> Option<String> {
    let bytes = base64url_decode(token)?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
fn base64url_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 0x3f) as usize] as char);
        }
    }
    out
}

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (k, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * k);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_an_absolute_path_with_slashes() {
        let path = "/home/dev/projects";
        let token = encode(path);
        assert!(!token.contains(['+', '/', '=']));
        assert_eq!(decode(&token).as_deref(), Some(path));
    }

    #[test]
    fn round_trips_paths_of_every_length_class() {
        for path in [
            "/",
            "/a",
            "/ab",
            "/abc",
            "/abcd",
            "/very/deep/nested/parent/dir",
        ] {
            let token = encode(path);
            assert_eq!(
                decode(&token).as_deref(),
                Some(path),
                "failed for {path:?}"
            );
        }
    }

    #[test]
    fn rejects_malformed_tokens() {
        assert_eq!(decode("not valid!"), None);
    }
}
