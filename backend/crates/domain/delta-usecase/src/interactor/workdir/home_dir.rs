use crate::error::{Error, Result};

/// The user's home directory, the default starting point for directory browsing.
///
/// Read from `HOME`. An absent or empty `HOME` leaves the picker with no
/// sensible default, so it is reported as an `InvalidWorkdir` rather than
/// browsing some arbitrary fallback.
pub(in crate::interactor::workdir) fn home_dir() -> Result<String> {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => Ok(home),
        _ => Err(Error::InvalidWorkdir(
            "HOME is not set; specify a path to browse".to_owned(),
        )),
    }
}
