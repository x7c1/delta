//! Correlation status of a recorded send.

use crate::error::{Error, Result};

/// Correlation status of a recorded send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendStatus {
    /// Recorded but not yet typed into the pane: it was composed while a turn
    /// was in flight, so its keystrokes are held back. Delta dispatches it
    /// (flipping it to `Dispatched`) once the session goes idle, so it submits
    /// as an ordinary prompt — the `UserPromptSubmit` hook fires and any locator
    /// quote is injected normally, rather than Claude Code queueing it mid-turn.
    Queued,
    /// Typed into the pane, awaiting a matching `UserPromptSubmit`.
    Dispatched,
    /// Matched to a transcript message uuid.
    Matched,
    /// Abandoned (e.g. superseded or timed out).
    Cancelled,
}

impl SendStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SendStatus::Queued => "queued",
            SendStatus::Dispatched => "dispatched",
            SendStatus::Matched => "matched",
            SendStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(SendStatus::Queued),
            "dispatched" => Ok(SendStatus::Dispatched),
            "matched" => Ok(SendStatus::Matched),
            "cancelled" => Ok(SendStatus::Cancelled),
            other => Err(Error::InvalidVariant {
                kind: "SendStatus",
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
            SendStatus::Queued,
            SendStatus::Dispatched,
            SendStatus::Matched,
            SendStatus::Cancelled,
        ] {
            assert_eq!(SendStatus::parse(s.as_str()).unwrap(), s);
        }
    }
}
