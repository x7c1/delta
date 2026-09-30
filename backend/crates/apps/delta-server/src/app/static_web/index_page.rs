//! The served `index.html`: the token tag injected into it, and the policy it
//! is sent with.

use axum::body::Bytes;
use include_dir::Dir;

/// The page's Content-Security-Policy, sent as a response header.
///
/// It mirrors the `<meta http-equiv>` policy in `index.html`, minus
/// `'unsafe-eval'`, which only Vite's dev client needs. Both policies are
/// enforced, so the header tightens the page, and unlike the `<meta>` tag it
/// also enforces `frame-ancestors`. `'unsafe-inline'` stays for the inline
/// theme bootstrap in `index.html`.
pub(super) const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; base-uri 'self'; \
     img-src 'self' data:; font-src 'self' data:; connect-src 'self'; \
     script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
     frame-ancestors 'none'; object-src 'none'";

/// The name of the tag the frontend reads the per-run token from.
const TOKEN_META_NAME: &str = "delta-auth-token";

/// `index.html` from `site` with the token tag injected before `</head>`.
pub(super) fn render(site: &Dir<'_>, token: &str) -> Bytes {
    let page = site
        .get_file("index.html")
        .and_then(|file| file.contents_utf8())
        .expect("the embedded web build has no UTF-8 index.html");
    let head_end = page
        .find("</head>")
        .expect("the embedded index.html has no </head> to inject the auth token before");
    let tag = format!(
        r#"<meta name="{TOKEN_META_NAME}" content="{}" />"#,
        escape_attribute(token)
    );
    let mut rendered = String::with_capacity(page.len() + tag.len());
    rendered.push_str(&page[..head_end]);
    rendered.push_str(&tag);
    rendered.push_str(&page[head_end..]);
    Bytes::from(rendered)
}

/// Escapes `value` for a double-quoted HTML attribute. A minted token is hex,
/// but `DELTA_AUTH_TOKEN` can be any string.
fn escape_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_attribute_neutralizes_markup() {
        assert_eq!(escape_attribute(r#"a"b<c>&d"#), "a&quot;b&lt;c&gt;&amp;d");
        assert_eq!(escape_attribute("deadbeef"), "deadbeef");
    }
}
