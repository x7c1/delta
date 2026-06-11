//! Lifecycle status of the single Claude Code session.

use crate::error::{Error, Result};

/// Lifecycle status of the single Claude Code session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Ended,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Ended => "ended",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "active" => Ok(SessionStatus::Active),
            "ended" => Ok(SessionStatus::Ended),
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
        for s in [SessionStatus::Active, SessionStatus::Ended] {
            assert_eq!(SessionStatus::parse(s.as_str()).unwrap(), s);
        }
    }

    #[test]
    fn invalid_status_is_error() {
        assert!(SessionStatus::parse("nope").is_err());
    }
}
