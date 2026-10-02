//! Where a link followed in the window goes.
//!
//! The page's links are ordinary anchors, most with `target="_blank"`. In a
//! browser those open a tab; the webview has no tabs, so they arrive as
//! new-window requests, and a link without a target would navigate the app's
//! only window away from Delta. The window builder therefore sorts both
//! new-window requests and navigations with [`classify`]:
//!
//! - a navigation within the app's own origin (`http://127.0.0.1:<port>`)
//!   stays in the window;
//! - a new-window request, even one for the app's own origin (a relative or
//!   footnote link), and a navigation to any other `http` / `https` URL are
//!   handed to the operating system's default opener ([`open_externally`]), so
//!   they open in the user's default browser;
//! - every other scheme is refused, so text in a message cannot open local
//!   files or other applications.

use std::process::{Command, Stdio};

use tauri::Url;

/// The loopback host the server is bound to and the window loads from.
const APP_HOST: &str = "127.0.0.1";

/// The kind of URL the page asked to navigate to or open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// The app's own origin.
    OwnOrigin,
    /// Any other `http` / `https` URL.
    Web,
    /// Any other scheme.
    NotWeb,
}

/// Classify `url` for an app served on `127.0.0.1:<port>`.
pub fn classify(url: &Url, port: u16) -> LinkKind {
    match url.scheme() {
        "http" if url.host_str() == Some(APP_HOST) && url.port() == Some(port) => {
            LinkKind::OwnOrigin
        }
        "http" | "https" => LinkKind::Web,
        _ => LinkKind::NotWeb,
    }
}

/// The operating system's command that opens a URL in the default browser.
#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const OPENER: &str = "xdg-open";

/// Open `url` with the operating system's default opener without blocking.
///
/// The opener runs as a child with the URL as its only argument, never through
/// a shell. A waiter thread reaps it and logs a non-zero exit; a failed spawn
/// is logged here.
pub fn open_externally(url: &Url) {
    let spawned = Command::new(OPENER)
        .arg(url.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            tracing::warn!(opener = OPENER, url = %url, "could not start the link opener: {err}");
            return;
        }
    };
    tracing::info!(opener = OPENER, url = %url, "opened link in the default browser");
    let url = url.clone();
    std::thread::spawn(move || match child.wait() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            tracing::warn!(opener = OPENER, url = %url, "link opener exited with {status}")
        }
        Err(err) => {
            tracing::warn!(opener = OPENER, url = %url, "could not wait for link opener: {err}")
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT: u16 = 51234;

    fn classify_str(url: &str) -> LinkKind {
        classify(&url.parse().expect("test URLs are valid"), PORT)
    }

    #[test]
    fn own_origin_is_recognised() {
        for url in [
            "http://127.0.0.1:51234/",
            "http://127.0.0.1:51234/sessions/42?tab=log#end",
        ] {
            assert_eq!(classify_str(url), LinkKind::OwnOrigin, "{url}");
        }
    }

    #[test]
    fn other_http_urls_are_web() {
        for url in [
            "https://github.com/x7c1/delta/pull/1",
            "http://example.com/",
            // Loopback, but not the app: another port, host or scheme.
            "http://127.0.0.1:7878/",
            "http://localhost:51234/",
            "https://127.0.0.1:51234/",
        ] {
            assert_eq!(classify_str(url), LinkKind::Web, "{url}");
        }
    }

    #[test]
    fn other_schemes_are_not_web() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "vscode://file/tmp/x",
            "mailto:someone@example.com",
            "about:blank",
            "data:text/html,hi",
        ] {
            assert_eq!(classify_str(url), LinkKind::NotWeb, "{url}");
        }
    }
}
