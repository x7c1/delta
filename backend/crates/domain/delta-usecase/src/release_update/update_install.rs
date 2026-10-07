/// The state of installing a downloaded update, once it was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateInstall {
    /// The installer runs, which includes the time the user takes to enter
    /// an administrator's password.
    Installing {
        /// The release being installed, `v<version>`.
        version: String,
    },
    /// Installed over the running app, which runs the new version once it
    /// restarts. Also when the installer found that version, or a newer one,
    /// installed already (from a terminal, say) and installed nothing.
    /// The downloaded file is kept.
    Installed {
        /// The release installed, `v<version>`.
        version: String,
    },
    /// The installer rejected the file itself (it does not match the
    /// release's digest, or is not Delta at the version asked for). The file
    /// was removed and the download no longer reads as ready: the user
    /// downloads the update again, and is never offered to install that file
    /// from a terminal. Cleared when a new download starts.
    Rejected {
        /// The release whose file was rejected, `v<version>`.
        version: String,
        /// Why, in the installer's words.
        cause: String,
    },
    /// The installer could not install the file for a reason other than the
    /// file itself (the release out of reach to check it against, the package
    /// manager failing). A new request tries again; the user can also install
    /// the file from a terminal.
    Failed {
        /// The release whose install failed, `v<version>`.
        version: String,
        /// Why, in the installer's words.
        cause: String,
        /// The command that installs the file from the user's own terminal
        /// ([`manual_install_command`](super::manual_install_command)).
        manual_command: String,
    },
    /// Delta cannot install updates itself on this machine (no polkit agent,
    /// not authorized, or the programs it needs are missing): the user
    /// installs the file from a terminal. A new request tries again.
    Unavailable {
        /// The release that is to be installed, `v<version>`.
        version: String,
        /// Why Delta cannot install it.
        cause: String,
        /// The command that installs the file from the user's own terminal.
        manual_command: String,
    },
}

impl UpdateInstall {
    /// The release this install is of, `v<version>`.
    pub fn version(&self) -> &str {
        match self {
            Self::Installing { version }
            | Self::Installed { version }
            | Self::Rejected { version, .. }
            | Self::Failed { version, .. }
            | Self::Unavailable { version, .. } => version,
        }
    }
}
