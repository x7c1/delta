//! Helper for declaring transparent string newtypes.
//!
//! Several identifiers are thin wrappers over Claude Code's string ids and
//! share the same conversions. Each id is defined alongside the model it
//! identifies (e.g. `SessionId` in `session`, `MessageUuid` in `message`); this
//! macro keeps the shared boilerplate in one place rather than repeating it.

/// Declares a transparent string newtype with the common conversions and
/// `Display`.
macro_rules! string_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(pub String);

        impl $name {
            /// Borrow the underlying string.
            pub fn as_str(&self) -> &str {
                &self.0
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

pub(crate) use string_newtype;
