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
    /// The launch ended without ever binding: it broke, the watchdog reaped it
    /// past its deadline, or the user closed the session while it was still
    /// starting.
    ///
    /// The row is kept in this state whether or not the session ingested
    /// anything. A never-bound launch ingested no messages, but it recorded
    /// everything else — the working directory, the repository and branch, the
    /// originating pull request, the prompt the user wrote, and (when Delta
    /// could name one) the failure reason — which is the whole of what someone
    /// needs to understand why their session did not start. So a failed launch
    /// is an ordinary thing the user can open, read, retry and remove, rather
    /// than a row that vanishes and leaves its failure nowhere to live.
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
