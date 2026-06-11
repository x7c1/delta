//! Correlation status of a queued send.

use crate::error::{Error, Result};

/// Correlation status of a queued send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingSendStatus {
    /// Held back because a turn was in flight when it was composed: its
    /// keystrokes have NOT been dispatched yet. Delta dispatches it (flipping it
    /// to `Pending`) once the session goes idle, so it submits as an ordinary
    /// prompt — the `UserPromptSubmit` hook fires and any locator quote is
    /// injected normally, rather than Claude Code queueing it mid-turn.
    Deferred,
    /// Dispatched, awaiting a matching `UserPromptSubmit`.
    Pending,
    /// Matched to a transcript message uuid.
    Matched,
    /// Abandoned (e.g. superseded or timed out).
    Cancelled,
}

impl PendingSendStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PendingSendStatus::Deferred => "deferred",
            PendingSendStatus::Pending => "pending",
            PendingSendStatus::Matched => "matched",
            PendingSendStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "deferred" => Ok(PendingSendStatus::Deferred),
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
            PendingSendStatus::Deferred,
            PendingSendStatus::Pending,
            PendingSendStatus::Matched,
            PendingSendStatus::Cancelled,
        ] {
            assert_eq!(PendingSendStatus::parse(s.as_str()).unwrap(), s);
        }
    }
}
