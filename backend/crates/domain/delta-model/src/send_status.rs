//! Correlation status of a recorded send.

use crate::error::{Error, Result};

/// Correlation status of a recorded send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendStatus {
    /// Recorded but not yet handed to the agent. For either provider it was
    /// accepted while a new session was still starting, before there was a
    /// pane or a provider thread to hand it to. For Claude it may also have
    /// been composed while a turn was in flight, so its keystrokes are held
    /// back until the session goes idle and it submits as an ordinary prompt —
    /// the `UserPromptSubmit` hook fires and any locator quote is injected
    /// normally, rather than Claude Code queueing it mid-turn. Delta dispatches
    /// it (flipping it to `Dispatched`) once the agent can take it.
    Queued,
    /// Handed to the agent, awaiting correlation: for Claude, typed into the
    /// pane and awaiting a matching `UserPromptSubmit`; for Codex, sent as a
    /// `turn/start` request and awaiting its acknowledgement.
    Dispatched,
    /// Correlated with the conversation (see `Send::matched_uuid`).
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
