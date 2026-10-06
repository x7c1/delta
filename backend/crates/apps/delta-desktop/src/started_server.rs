//! What the window's link handlers learn from the background start.
//!
//! The window is built before the server listens, so the handlers it is built
//! with cannot capture the server's port or the login shell's environment.
//! They share a [`StartedServer`] instead, which the background start fills in
//! once, right before it navigates the window to the server. Until then the
//! window shows only the placeholder page.

use std::sync::OnceLock;

/// The started server's port and the environment links are opened with, once
/// the server listens.
#[derive(Debug, Default)]
pub struct StartedServer(OnceLock<Started>);

#[derive(Debug)]
struct Started {
    port: u16,
    opener_env: Vec<(String, String)>,
}

impl StartedServer {
    /// The port the server listens on, or `None` while it is still starting.
    pub fn port(&self) -> Option<u16> {
        self.0.get().map(|started| started.port)
    }

    /// The login shell's `PATH` and locale, set on the link opener; empty
    /// while the server is still starting (the placeholder page has no links).
    pub fn opener_env(&self) -> &[(String, String)] {
        self.0
            .get()
            .map_or(&[], |started| started.opener_env.as_slice())
    }

    /// Record the started server. The start runs once, so a second record is
    /// a bug; it is logged and ignored.
    pub fn record(&self, port: u16, opener_env: Vec<(String, String)>) {
        if self.0.set(Started { port, opener_env }).is_err() {
            tracing::error!(
                port,
                "the server was recorded as started twice; keeping the first record"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_before_the_start() {
        let server = StartedServer::default();
        assert_eq!(server.port(), None);
        assert!(server.opener_env().is_empty());
    }

    #[test]
    fn known_after_the_start() {
        let server = StartedServer::default();
        let env = vec![("PATH".to_owned(), "/opt/bin:/usr/bin".to_owned())];
        server.record(51234, env.clone());
        assert_eq!(server.port(), Some(51234));
        assert_eq!(server.opener_env(), env.as_slice());
    }

    #[test]
    fn a_second_record_keeps_the_first() {
        let server = StartedServer::default();
        server.record(51234, Vec::new());
        server.record(7878, vec![("LANG".to_owned(), "C".to_owned())]);
        assert_eq!(server.port(), Some(51234));
        assert!(server.opener_env().is_empty());
    }
}
