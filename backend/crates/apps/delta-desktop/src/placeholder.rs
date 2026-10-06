//! The page the window shows while the server starts.
//!
//! The window opens before the server is up, on one static page served by the
//! shell itself under its own URI scheme ([`SCHEME`]) at one path:
//! `delta://localhost/starting` on macOS and Linux, where Tauri hands a custom
//! scheme's URL to the webview as it is. A scheme rather than a `data:` URL,
//! so [`links::classify`](crate::links::classify) can name the page as the
//! shell's own ([`is_placeholder`]) and keep it in the window. Once the server
//! listens, the window navigates away from it to the server.

use std::borrow::Cow;

use tauri::http::{header, HeaderValue, Request, Response, StatusCode};
use tauri::Url;

/// The URI scheme the placeholder is served under, registered with
/// `Builder::register_uri_scheme_protocol`.
pub const SCHEME: &str = "delta";

/// The host the webview gives a custom scheme's URLs on macOS and Linux.
const HOST: &str = "localhost";

/// The only path the scheme serves.
const PATH: &str = "/starting";

/// The page: the app's background, light or dark as the OS is, and one line
/// of text.
const PAGE: &str = include_str!("placeholder.html");

/// The placeholder page's URL.
pub fn url() -> anyhow::Result<Url> {
    Ok(format!("{SCHEME}://{HOST}{PATH}").parse()?)
}

/// Whether `url` is on the placeholder's origin.
pub fn is_placeholder(url: &Url) -> bool {
    url.scheme() == SCHEME && url.host_str() == Some(HOST)
}

/// Whether a navigation to `url` would take the window back to the placeholder
/// once the server listens on `port`: the webview's Back (a context-menu item
/// or a mouse button) from the server's first page. The page would then say
/// "Starting Delta…" for good, since nothing navigates the window on again.
pub fn is_return_after_start(url: &Url, port: Option<u16>) -> bool {
    port.is_some() && is_placeholder(url)
}

/// Answer a request to the scheme: the page at its path, 404 anywhere else.
pub fn respond(request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    if path != PATH {
        tracing::warn!(
            path,
            "request for a path the placeholder scheme does not serve"
        );
        let mut response = Response::new(Cow::Borrowed(&b"not found"[..]));
        *response.status_mut() = StatusCode::NOT_FOUND;
        return response;
    }
    let mut response = Response::new(Cow::Borrowed(PAGE.as_bytes()));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(uri: &str) -> Request<Vec<u8>> {
        let mut request = Request::new(Vec::new());
        *request.uri_mut() = uri.parse().expect("test URIs are valid");
        request
    }

    #[test]
    fn the_url_is_the_placeholder() {
        let url = url().expect("the placeholder URL is valid");
        assert_eq!(url.as_str(), "delta://localhost/starting");
        assert!(is_placeholder(&url));
    }

    #[test]
    fn serves_the_page_at_its_path() {
        let url = url().expect("the placeholder URL is valid");
        let response = respond(&request(url.as_str()));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("text/html; charset=utf-8"))
        );
        let body = std::str::from_utf8(response.body()).expect("the page is UTF-8");
        assert!(body.contains("Starting Delta…"), "{body}");
    }

    #[test]
    fn other_paths_are_not_found() {
        for uri in ["delta://localhost/", "delta://localhost/favicon.ico"] {
            assert_eq!(
                respond(&request(uri)).status(),
                StatusCode::NOT_FOUND,
                "{uri}"
            );
        }
    }

    #[test]
    fn returning_to_the_placeholder_counts_only_after_the_start() {
        let url = url().expect("the placeholder URL is valid");
        assert!(!is_return_after_start(&url, None));
        assert!(is_return_after_start(&url, Some(51234)));
        let server: Url = "http://127.0.0.1:51234/".parse().expect("valid URL");
        assert!(!is_return_after_start(&server, Some(51234)));
    }

    #[test]
    fn other_origins_are_not_the_placeholder() {
        for url in [
            "http://127.0.0.1:51234/starting",
            "delta://example.com/starting",
            "https://localhost/starting",
        ] {
            let url: Url = url.parse().expect("test URLs are valid");
            assert!(!is_placeholder(&url), "{url}");
        }
    }
}
