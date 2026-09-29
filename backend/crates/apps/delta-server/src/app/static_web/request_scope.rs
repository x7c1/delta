//! Which requests the static surface may answer at all.

use axum::http::{header, HeaderMap};

/// Path prefixes that belong to the server's own surface. A request under one
/// of these is never answered with the page, so an unknown API path keeps its
/// `404` instead of turning into HTML.
pub(super) const RESERVED_PREFIXES: &[&str] =
    &["/api", "/ws", "/pty", "/comms", "/hooks", "/health"];

/// True when `path` is one of [`RESERVED_PREFIXES`] or lies under one.
pub(super) fn is_reserved(path: &str) -> bool {
    RESERVED_PREFIXES.iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// True when the request's `Accept` header admits `text/html` — what a browser
/// navigation sends, and what a script's `fetch` or an asset request does not.
pub(super) fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|media_range| {
            let media_type = media_range.split(';').next().unwrap_or("").trim();
            media_type.eq_ignore_ascii_case("text/html")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn reserved_prefixes_match_whole_segments_only() {
        for path in [
            "/api",
            "/api/sessions",
            "/ws",
            "/pty",
            "/comms",
            "/hooks/stop",
            "/health",
        ] {
            assert!(is_reserved(path), "{path} is reserved");
        }
        for path in [
            "/",
            "/sessions/xyz",
            "/apiary",
            "/healthy",
            "/assets/app.js",
        ] {
            assert!(!is_reserved(path), "{path} is not reserved");
        }
    }

    #[test]
    fn accepts_html_reads_media_ranges() {
        let with = |accept: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::ACCEPT, HeaderValue::from_str(accept).unwrap());
            accepts_html(&headers)
        };
        assert!(with("text/html,application/xhtml+xml,*/*;q=0.8"));
        assert!(with("TEXT/HTML; q=0.9"));
        assert!(!with("application/json"));
        assert!(!with("*/*"));
        assert!(!accepts_html(&HeaderMap::new()));
    }
}
