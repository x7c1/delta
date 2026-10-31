//! Where a link followed in the window goes.
//!
//! The page's links are ordinary anchors, most with `target="_blank"`. In a
//! browser those open a tab; the webview has no tabs, so they arrive as
//! new-window requests, and a link without a target would navigate the app's
//! only window away from Delta. The window builder therefore sorts both
//! new-window requests and navigations with [`classify`]:
//!
//! - a navigation within the app's own origin — the server's
//!   `http://127.0.0.1:<port>` once it listens (including the URL with the
//!   one-time code in its fragment the window is opened at), and the
//!   placeholder page the window shows until then ([`placeholder`]) — stays in
//!   the window;
//! - a new-window request for an `http` / `https` URL, even one for the app's
//!   own origin (a relative or footnote link), and a navigation to any other
//!   `http` / `https` URL are handed to the operating system's default opener
//!   ([`open_externally`]), so they open in the user's default browser;
//! - every other scheme is refused, so text in a message cannot open local
//!   files or other applications.
//!
//! While the server is still starting its port is unknown, so no `http` URL is
//! the app's own yet: every one goes to the browser.
//!
//! On Linux the opener is handed an activation token minted for Delta's window
//! (`activation_token::token_for_launch`), so a Wayland compositor lets the
//! browser's window come to the front rather than announce it in a
//! notification. The webview's new-window and navigation handlers run on the
//! GTK main thread (WebKitGTK's policy decisions), where the token is minted
//! at once; minting on any other thread would ask the main thread for it.
//! Without a token the link opens as before.

use std::process::{Command, Stdio};

use tauri::Url;

use crate::placeholder;

/// The loopback host the server is bound to and the window loads from.
const APP_HOST: &str = "127.0.0.1";

/// The kind of URL the page asked to navigate to or open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// The app's own origin: the server's, or the placeholder page's.
    OwnOrigin,
    /// Any other `http` / `https` URL.
    Web,
    /// Any other scheme.
    NotWeb,
}

/// Classify `url` for an app served on `127.0.0.1:<port>`, or still starting
/// (`port` is `None`) behind the placeholder page.
pub fn classify(url: &Url, port: Option<u16>) -> LinkKind {
    if placeholder::is_placeholder(url) {
        return LinkKind::OwnOrigin;
    }
    match url.scheme() {
        "http" if port.is_some() && url.host_str() == Some(APP_HOST) && url.port() == port => {
            LinkKind::OwnOrigin
        }
        "http" | "https" => LinkKind::Web,
        _ => LinkKind::NotWeb,
    }
}

/// The operating system's command that opens a URL in the default browser.
#[cfg(target_os = "macos")]
pub const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
pub const OPENER: &str = "xdg-open";

/// Open `url` with the operating system's default opener without blocking.
///
/// The opener runs as a child with the URL as its only argument, never through
/// a shell, with `env` (the login shell's `PATH` and locale) set on top of the
/// inherited environment, and the activation `token` when there is one. A
/// waiter thread reaps it and logs a non-zero exit; a failed spawn is logged
/// here.
pub fn open_externally(url: &Url, env: &[(String, String)], token: Option<&str>) {
    let spawned = opener_command(url, env, token).spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            tracing::warn!(opener = OPENER, url = %url, "could not start the link opener: {err}");
            return;
        }
    };
    tracing::info!(
        opener = OPENER,
        url = %url,
        activation_token = token.is_some(),
        "opened link in the default browser"
    );
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

/// The opener command for `url`: the URL its only argument, `env` and the
/// activation `token` (in both [`delta_bootstrap::ACTIVATION_TOKEN_VARS`]) set, stdio
/// suppressed.
fn opener_command(url: &Url, env: &[(String, String)], token: Option<&str>) -> Command {
    let mut command = Command::new(OPENER);
    command
        .arg(url.as_str())
        .envs(env.iter().map(|(name, value)| (name, value)));
    if let Some(token) = token {
        command.envs(delta_bootstrap::ACTIVATION_TOKEN_VARS.map(|name| (name, token)));
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The variables set on the opener command for a link, sorted by name.
    fn opener_env(token: Option<&str>) -> Vec<(String, Option<String>)> {
        let url: Url = "https://example.com/".parse().unwrap();
        let env = [("LANG".to_owned(), "C".to_owned())];
        let command = opener_command(&url, &env, token);
        let mut envs: Vec<_> = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        envs.sort();
        envs
    }

    #[test]
    fn the_opener_carries_the_activation_token() {
        assert_eq!(
            opener_env(Some("token-1")),
            [
                ("DESKTOP_STARTUP_ID".to_owned(), Some("token-1".to_owned())),
                ("LANG".to_owned(), Some("C".to_owned())),
                (
                    "XDG_ACTIVATION_TOKEN".to_owned(),
                    Some("token-1".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn without_a_token_the_opener_carries_no_activation_variables() {
        assert_eq!(
            opener_env(None),
            [("LANG".to_owned(), Some("C".to_owned()))]
        );
    }

    #[test]
    fn the_opener_is_given_the_url_alone() {
        let url: Url = "https://example.com/a b".parse().unwrap();
        let command = opener_command(&url, &[], None);
        assert_eq!(command.get_program(), OPENER);
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, [url.as_str()]);
    }

    const PORT: u16 = 51234;

    fn classify_str(url: &str) -> LinkKind {
        classify_while(url, Some(PORT))
    }

    fn classify_while(url: &str, port: Option<u16>) -> LinkKind {
        classify(&url.parse().expect("test URLs are valid"), port)
    }

    #[test]
    fn own_origin_is_recognised() {
        for url in [
            "http://127.0.0.1:51234/",
            "http://127.0.0.1:51234/sessions/42?tab=log#end",
            // The URL the window opens, with its one-time code.
            "http://127.0.0.1:51234/#code=0123abcd",
        ] {
            assert_eq!(classify_str(url), LinkKind::OwnOrigin, "{url}");
        }
    }

    #[test]
    fn the_placeholder_is_own_origin_before_and_after_the_start() {
        let url = placeholder::url().expect("the placeholder URL is valid");
        for port in [None, Some(PORT)] {
            assert_eq!(
                classify(&url, port),
                LinkKind::OwnOrigin,
                "{url} with port {port:?}"
            );
        }
    }

    #[test]
    fn before_the_start_no_http_url_is_own_origin() {
        for url in ["http://127.0.0.1:51234/", "http://127.0.0.1:7878/"] {
            assert_eq!(classify_while(url, None), LinkKind::Web, "{url}");
        }
        for url in ["file:///etc/passwd", "delta://example.com/starting"] {
            assert_eq!(classify_while(url, None), LinkKind::NotWeb, "{url}");
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
