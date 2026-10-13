//! Who assigns the provider's session identity.

/// Who assigns the provider's session identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdentityCapability {
    /// Delta mints the id and pins it at launch (Claude's `--session-id`).
    DeltaCanSetId,
    /// The provider assigns the id and returns it (Codex's `thr_...`).
    ProviderReturnsId,
}
