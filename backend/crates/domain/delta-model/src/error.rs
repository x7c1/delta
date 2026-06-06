//! Crate-local error type.
//!
//! The domain layer has very few fallible operations (it is mostly plain data),
//! but it still defines its own `Error`/`Result` so that callers in higher
//! layers can convert into it along the dependency direction.

use std::fmt;

/// Errors raised while constructing or validating domain values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A string did not parse into the expected enum variant.
    InvalidVariant { kind: &'static str, value: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidVariant { kind, value } => {
                write!(f, "invalid {kind} value: {value:?}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
