//! [`Secret`]: a credential whose `Debug` output hides it.

use std::fmt;
use std::ops::Deref;

/// A credential the configuration holds — the bearer token, the hook secret —
/// that must never reach a log line.
///
/// Its `Debug` prints `***`, so a `?config` (or any `{:?}` of a value that
/// carries one) cannot print it. It has no `Display`, so it cannot be logged
/// with `%` either. The value is read as a `&str` through `Deref`, which is
/// how the code that compares or renders it gets at it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl Deref for Secret {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl PartialEq<str> for Secret {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Secret {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_value() {
        let secret = Secret::from("s3cr3t-value");
        assert_eq!(format!("{secret:?}"), "***");
        assert_eq!(&*secret, "s3cr3t-value");
    }
}
