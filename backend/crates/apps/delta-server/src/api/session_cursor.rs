//! Opaque encoding for the session-list page cursor.
//!
//! The browser pages `GET /api/sessions` by echoing back the `next_cursor` the
//! previous page returned. That token is deliberately opaque: callers must not
//! parse or construct it, so its internal shape (the three sort keys) stays free
//! to change. Here it is encoded as compact JSON, then base64url (URL-safe, no
//! padding) so it travels cleanly in a query string. Decoding a malformed token
//! is a client error, surfaced as `400` by the handler.

use delta_usecase::SessionPageCursor;
use serde::{Deserialize, Serialize};

/// The wire form of a [`SessionPageCursor`]: the three sort keys, JSON-encoded
/// before base64url. Field names are short to keep the token compact; they are
/// an implementation detail of this opaque token, never a public contract.
#[derive(Serialize, Deserialize)]
struct CursorWire {
    /// `recency` — last activity, or `created_at` fallback.
    r: String,
    /// `created_at`.
    c: String,
    /// `id`.
    i: String,
}

/// Encode a cursor into an opaque base64url token.
pub(crate) fn encode(cursor: &SessionPageCursor) -> String {
    let wire = CursorWire {
        r: cursor.recency.clone(),
        c: cursor.created_at.clone(),
        i: cursor.id.clone(),
    };
    // JSON serialization of three owned strings cannot fail.
    let json = serde_json::to_vec(&wire).expect("cursor serializes to JSON");
    base64url_encode(&json)
}

/// Decode an opaque token back into a cursor, or `None` if it is malformed
/// (bad base64url or JSON that does not match the cursor shape). The caller maps
/// `None` to an HTTP `400`.
pub(crate) fn decode(token: &str) -> Option<SessionPageCursor> {
    let bytes = base64url_decode(token)?;
    let wire: CursorWire = serde_json::from_slice(&bytes).ok()?;
    Some(SessionPageCursor {
        recency: wire.r,
        created_at: wire.c,
        id: wire.i,
    })
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url (RFC 4648 §5), no padding.
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

/// Decode base64url (no padding). Returns `None` on any invalid character or a
/// malformed length (a lone trailing sextet carries no whole byte).
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
            // A single sextet decodes to zero bytes — an invalid length.
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

    fn cursor() -> SessionPageCursor {
        SessionPageCursor {
            recency: "2026-02-01T00:00:00Z".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            id: "sess-1".into(),
        }
    }

    #[test]
    fn round_trips_a_cursor() {
        let token = encode(&cursor());
        // The token is URL-safe: no '+', '/', or '=' padding.
        assert!(!token.contains(['+', '/', '=']));
        assert_eq!(decode(&token), Some(cursor()));
    }

    #[test]
    fn round_trips_inputs_of_every_length_class() {
        // Cover each chunk remainder (0, 1, 2 bytes) so encode/decode agree on
        // the trailing-group handling.
        for id in ["", "a", "ab", "abc", "abcd"] {
            let c = SessionPageCursor {
                recency: "r".into(),
                created_at: "c".into(),
                id: id.into(),
            };
            let token = encode(&c);
            assert_eq!(decode(&token), Some(c), "failed for id {id:?}");
        }
    }

    #[test]
    fn rejects_malformed_tokens() {
        // Invalid base64url character.
        assert_eq!(decode("not valid!"), None);
        // Valid base64url, but the bytes are not the cursor JSON shape.
        let garbage = base64url_encode(b"\xff\xff\xff");
        assert_eq!(decode(&garbage), None);
    }
}
