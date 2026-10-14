//! The served `index.html`, and the policy it is sent with.

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

/// `index.html` from `site`, exactly as the build wrote it.
pub(super) fn page(site: &'static Dir<'static>) -> Bytes {
    let page = site
        .get_file("index.html")
        .expect("the embedded web build has no index.html");
    Bytes::from_static(page.contents())
}
