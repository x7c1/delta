//! Correlation status of a queued send.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Correlation status of a queued send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PendingSendStatus {
    /// Queued, awaiting a matching `UserPromptSubmit`.
    Pending,
    /// Matched to a transcript message uuid.
    Matched,
    /// Abandoned (e.g. superseded or timed out).
    Cancelled,
}

impl PendingSendStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PendingSendStatus::Pending => "pending",
            PendingSendStatus::Matched => "matched",
            PendingSendStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(PendingSendStatus::Pending),
            "matched" => Ok(PendingSendStatus::Matched),
            "cancelled" => Ok(PendingSendStatus::Cancelled),
            other => Err(Error::InvalidVariant {
                kind: "PendingSendStatus",
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
            PendingSendStatus::Pending,
            PendingSendStatus::Matched,
            PendingSendStatus::Cancelled,
        ] {
            assert_eq!(PendingSendStatus::parse(s.as_str()).unwrap(), s);
        }
    }
}
