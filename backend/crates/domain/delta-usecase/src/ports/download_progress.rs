//! How far a running asset download has got.

/// How far a running download has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    /// Bytes received so far.
    pub received: u64,
    /// The whole file's size, when the answer stated it.
    pub total: Option<u64>,
}
