//! Where a permission decision is made.

/// Where a permission decision is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionCapability {
    /// The adapter answers the request directly (a structured RPC reply).
    AdapterDecision,
    /// The decision rides a hook response back to the provider (Claude).
    HookDecision,
    /// The provider owns its own permission UI; Delta cannot answer.
    ProviderUiOnly,
}
