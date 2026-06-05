//! Identifier of a thread (an overlay Delta owns, issued by the store).

use std::fmt;

use serde::{Deserialize, Serialize};

/// Identifier of a thread (an overlay Delta owns, issued by the store).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadId(pub i64);

impl ThreadId {
    /// Borrow the underlying integer value.
    pub fn value(self) -> i64 {
        self.0
    }
}

impl From<i64> for ThreadId {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
