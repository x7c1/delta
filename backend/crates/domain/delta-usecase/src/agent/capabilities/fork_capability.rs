//! Whether the provider can fork a conversation into an independent branch.

/// Whether the provider can fork a conversation into an independent branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkCapability {
    /// No native fork. All providers are `None` in v1.
    None,
    /// The provider forks natively into a new thread (reserved for future use).
    NativeThreadFork,
}
