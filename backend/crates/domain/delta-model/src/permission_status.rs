//! Disposition of a recorded permission request.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Disposition of a recorded permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionStatus {
    /// Awaiting the user's decision in the TUI.
    Pending,
    Allowed,
    Denied,
}

impl PermissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionStatus::Pending => "pending",
            PermissionStatus::Allowed => "allowed",
            PermissionStatus::Denied => "denied",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(PermissionStatus::Pending),
            "allowed" => Ok(PermissionStatus::Allowed),
            "denied" => Ok(PermissionStatus::Denied),
            other => Err(Error::InvalidVariant {
                kind: "PermissionStatus",
                value: other.to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_enum_round_trips() {
        for s in [
            PermissionStatus::Pending,
            PermissionStatus::Allowed,
            PermissionStatus::Denied,
        ] {
            assert_eq!(PermissionStatus::parse(s.as_str()).unwrap(), s);
        }
    }
}
