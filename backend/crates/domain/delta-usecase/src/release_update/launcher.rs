//! Which shell launched the server.

/// Which shell launched the server.
///
/// Only the desktop app can be replaced by a release's bundle; the CLI server
/// (the browser version) is installed and updated some other way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launcher {
    /// The `delta-server` binary, served to a browser.
    Cli,
    /// The desktop app, which runs the server in-process.
    Desktop,
}
