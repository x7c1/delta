//! JSON shape of a message's author role.

use delta_model::Role;
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of a message's author role.
///
/// Mirrors the domain [`Role`] variant-for-variant; see that type for the
/// semantics of each role. This wire twin carries the serialization concerns
/// the domain type must not know about: the snake_case variant names and the
/// TypeScript export. The rename is `snake_case` (not `lowercase`) so the
/// multi-word `compact_summary` round-trips its [`delta_model::Role::as_str`]
/// label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "MessageRole")]
pub enum WireRole {
    User,
    Assistant,
    System,
    Meta,
    CompactSummary,
    Other,
}

impl From<Role> for WireRole {
    fn from(role: Role) -> Self {
        match role {
            Role::User => WireRole::User,
            Role::Assistant => WireRole::Assistant,
            Role::System => WireRole::System,
            Role::Meta => WireRole::Meta,
            Role::CompactSummary => WireRole::CompactSummary,
            Role::Other => WireRole::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(WireRole::from(Role::Meta)).unwrap(),
            serde_json::json!("meta"),
        );
        // The multi-word variant pins the `snake_case` rename (`lowercase`
        // would render this as `compactsummary`, drifting from the domain
        // `Role::as_str` label and the database storage).
        assert_eq!(
            serde_json::to_value(WireRole::from(Role::CompactSummary)).unwrap(),
            serde_json::json!("compact_summary"),
        );
    }
}
