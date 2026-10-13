//! Whether a closed session can be reopened with its history.

/// Whether a closed session can be reopened with its history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeCapability {
    /// Resume is supported.
    Supported,
    /// Resume is not supported.
    Unsupported,
}
