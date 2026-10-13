//! The platforms the desktop app is built for.

/// The platforms the desktop app is built for, as far as where the shell's
/// files live is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// WKWebView keeps its storage outside the app data directory, in
    /// `~/Library/WebKit/<identifier>`.
    MacOs,
    /// WebKitGTK keeps its storage inside the app data directory.
    Linux,
}

impl Platform {
    /// The platform this build runs on.
    pub const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::MacOs
    } else {
        Self::Linux
    };
}
