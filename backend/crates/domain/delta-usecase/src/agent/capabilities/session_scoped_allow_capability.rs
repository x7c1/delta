//! Whether the provider understands a session-scoped allow.

#[cfg(doc)]
use super::PermissionCapability;

/// Whether the provider understands an allow that stands for the rest of the
/// session, rather than only for the one request being answered.
///
/// Deliberately *not* folded into [`PermissionCapability`]: that one answers
/// **where** a decision is made (adapter reply, hook response, or nowhere Delta
/// can reach), which is a different question from **which decisions exist**. A
/// provider could answer over an adapter and still know only per-request
/// decisions, so the two vary independently and are declared independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionScopedAllowCapability {
    /// The provider accepts an allow scoped to the whole session, and stops
    /// asking for comparable requests afterwards (Codex's `acceptForSession`).
    /// The scope lives entirely in the provider's session — Delta neither
    /// records which grants are outstanding nor replays them.
    Supported,
    /// Every decision answers exactly one request (Claude's permission hook,
    /// whose response `behavior` has no session-scoped form). Delta rejects a
    /// session-scoped decision for such a provider rather than degrading it
    /// into a plain allow, which would keep prompting a user who asked not to
    /// be prompted, with nothing said about why.
    Unsupported,
}
