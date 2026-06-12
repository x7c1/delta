//! Lifecycle status of a Claude Code session.

use crate::error::{Error, Result};

/// Lifecycle status of a Claude Code session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// The session row was created when Delta minted the id and launched
    /// `claude`, but no hook has bound the spawn yet (the transcript path is
    /// still unknown).
    Spawning,
    Active,
    Ended,
    /// The spawn never bound before its deadline. A failed session that
    /// ingested no messages is deleted at reap time instead of being kept in
    /// this state, so `Failed` only survives for a session that already holds
    /// data worth keeping.
    Failed,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Spawning => "spawning",
            SessionStatus::Active => "active",
            SessionStatus::Ended => "ended",
            SessionStatus::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "spawning" => Ok(SessionStatus::Spawning),
            "active" => Ok(SessionStatus::Active),
            "ended" => Ok(SessionStatus::Ended),
            "failed" => Ok(SessionStatus::Failed),
            other => Err(Error::InvalidVariant {
                kind: "SessionStatus",
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
            SessionStatus::Spawning,
            SessionStatus::Active,
            SessionStatus::Ended,
            SessionStatus::Failed,
        ] {
            assert_eq!(SessionStatus::parse(s.as_str()).unwrap(), s);
        }
    }

    #[test]
    fn invalid_status_is_error() {
        assert!(SessionStatus::parse("nope").is_err());
    }
}
