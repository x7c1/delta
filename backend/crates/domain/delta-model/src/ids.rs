//! Strongly-typed identifiers.
//!
//! Claude Code assigns each transcript line a `uuid`; Delta uses that uuid as
//! the internal handle for a message. There is no separate human-readable
//! sequence layer. Threads are an overlay Delta owns, so their ids are plain
//! integers issued by the store.

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! string_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Borrow the underlying string.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume into the underlying string.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_newtype! {
    /// Identifier of the single Claude Code session (`session_id` from hooks).
    SessionId
}

string_newtype! {
    /// A transcript line uuid; the internal handle for a message.
    MessageUuid
}

string_newtype! {
    /// Claude Code's `promptId`, shared by all lines of one turn.
    PromptId
}

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
