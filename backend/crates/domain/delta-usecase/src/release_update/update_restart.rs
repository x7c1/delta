use crate::ports::InstalledApp;

/// A restart into the update installed over the running app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRestart {
    /// The release installed, `v<version>`.
    pub version: String,
    /// The installed app to start again, as the installer named it when it
    /// installed the update.
    pub app: InstalledApp,
}
