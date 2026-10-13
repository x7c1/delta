//! Why an update was not installed.

use std::path::PathBuf;

/// Why an update was not installed.
///
/// Its text names only its own level; the errors it was caused by are its
/// `source` chain, to be rendered once with [`crate::with_causes()`].
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// The user dismissed the request for an administrator's password.
    /// Nothing went wrong: the update stays ready to install.
    #[error("the request for an administrator's password was dismissed")]
    Dismissed,
    /// Delta cannot install the update itself on this machine: on Linux, the
    /// program that asks for the password or the one that installs is
    /// missing, no authentication agent is running, or the user is not
    /// authorized; on macOS, the app does not run from a `Delta.app` it may
    /// replace, or `hdiutil` is missing. The user can still install it by
    /// hand.
    #[error("Delta cannot install the update itself")]
    Unavailable {
        /// Why: the installer's own error, boxed, as this crate cannot name
        /// the installers' error types (a caller may downcast it, and reach
        /// the error it was caused by through its `source`); text where all
        /// the installer has is text.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The program the install needs is there but could not be started.
    /// Delta cannot install the update itself, as for
    /// [`Self::Unavailable`]; the user can still install it by hand.
    #[error("Delta cannot install the update itself: could not run {}", program.display())]
    Unrunnable {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The installer rejected the file itself: not a regular file, not the
    /// one the release's digest names, or not Delta at the version it was
    /// asked for. The file is not to be installed by any means, by hand
    /// included.
    #[error("{reason}")]
    Rejected {
        /// The installer's own one-line reason, without its `source`.
        reason: String,
        /// The error the installer met, where it has one as a value (boxed,
        /// as this crate cannot name the installers' error types; a caller
        /// may downcast it); `None` where all it has is text, such as a line
        /// another process printed.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    /// The installer ran and could not install the file for a reason other
    /// than the file itself (the release out of reach to check it against,
    /// the file not copied to where it is checked, the package manager or a
    /// disk image tool failing, the mounted disk image's contents not
    /// listed, the new app bundle not renamed into place). The user can
    /// still install it by hand.
    #[error("{reason}")]
    Failed {
        /// As for [`Self::Rejected`].
        reason: String,
        /// As for [`Self::Rejected`].
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl InstallError {
    /// [`Self::Unavailable`] because of `source`: an error value, or text
    /// where there is no value to keep.
    pub fn unavailable(source: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self::Unavailable {
            source: source.into(),
        }
    }

    /// [`Self::Rejected`] for `reason` alone.
    pub fn rejected(reason: impl Into<String>) -> Self {
        Self::Rejected {
            reason: reason.into(),
            source: None,
        }
    }

    /// [`Self::Rejected`] for `reason`, caused by `source`.
    pub fn rejected_with(
        reason: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Rejected {
            reason: reason.into(),
            source: Some(Box::new(source)),
        }
    }

    /// [`Self::Failed`] for `reason` alone.
    pub fn failed(reason: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
            source: None,
        }
    }

    /// [`Self::Failed`] for `reason`, caused by `source`.
    pub fn failed_with(
        reason: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Failed {
            reason: reason.into(),
            source: Some(Box::new(source)),
        }
    }
}
