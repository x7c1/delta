//! The author role of a message.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The author role of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    /// A transcript line whose kind Delta does not classify (e.g. summaries).
    Other,
}

impl Role {
    /// Parse a transcript `type` string into a role.
    ///
    /// Unknown kinds map to [`Role::Other`] rather than failing, because linear
    /// parent chains can include line kinds Delta does not model.
    pub fn from_transcript_type(value: &str) -> Self {
        match value {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "system" => Role::System,
            _ => Role::Other,
        }
    }

    /// The canonical lowercase label stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Other => "other",
        }
    }

    /// Parse a stored role label back into a [`Role`].
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "user" => Ok(Role::User),
            "assistant" => Ok(Role::Assistant),
            "system" => Ok(Role::System),
            "other" => Ok(Role::Other),
            other => Err(Error::InvalidVariant {
                kind: "Role",
                value: other.to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_round_trips_through_string() {
        for role in [Role::User, Role::Assistant, Role::System, Role::Other] {
            assert_eq!(Role::parse(role.as_str()).unwrap(), role);
        }
    }

    #[test]
    fn unknown_transcript_type_is_other_not_error() {
        assert_eq!(Role::from_transcript_type("summary"), Role::Other);
        assert_eq!(Role::from_transcript_type("assistant"), Role::Assistant);
    }
}
