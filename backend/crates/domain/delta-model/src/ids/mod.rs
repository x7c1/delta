//! Strongly-typed identifiers.
//!
//! Claude Code assigns each transcript line a `uuid`; Delta uses that uuid as
//! the internal handle for a message. There is no separate human-readable
//! sequence layer. Threads are an overlay Delta owns, so their ids are plain
//! integers issued by the store.

/// Declares a transparent string newtype with the common conversions and
/// `Display`. Each id below lives in its own module and invokes this once.
macro_rules! string_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
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

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

mod message_uuid;
pub use message_uuid::MessageUuid;
mod prompt_id;
pub use prompt_id::PromptId;
mod session_id;
pub use session_id::SessionId;
mod thread_id;
pub use thread_id::ThreadId;
