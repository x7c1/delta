//! [`DataLayout`]: every path Delta writes, derived from its data directory.
//!
//! The server owns one **data directory** (`Config::data_dir`) and derives
//! every file and directory it writes from it, here and nowhere else:
//!
//! ```text
//! <data_dir>/
//!   delta.db                  the SQLite overlay (SQLite adds `-wal`/`-shm`
//!                             beside it, the migration runner `.bak-v<N>`)
//!   delta-hook-state.json     the hook secret and the desktop app's port
//!   sessions/<token>/         per-spawn working directories
//!   settings/<port>.json      the Claude Code session settings (hook URLs)
//!   tmux.conf                 the configuration Delta's tmux server loads
//!   updates/                  the desktop app's downloaded, verified release
//! ```
//!
//! Per-session git worktrees are deliberately *not* here: they are paths the
//! user sees, which git metadata and Claude Code's trust configuration record,
//! so they keep their own base (`Config::worktree_base`).
//!
//! A value of its own rather than methods on [`Config`]: the shells, the
//! gateways and anything that later lists or removes Delta's files need the
//! paths without the rest of the configuration (secrets, launch settings), and
//! one type is the single place a new file is added.
//!
//! [`Config`]: crate::Config

use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};

use crate::DataDirError;

/// The SQLite overlay's file name.
const DATABASE_FILE: &str = "delta.db";

/// The hook state file's name.
const HOOK_STATE_FILE: &str = "delta-hook-state.json";

/// The per-spawn working-directory base's name.
const SESSIONS_DIR: &str = "sessions";

/// The directory holding one session settings file per port.
const SETTINGS_DIR: &str = "settings";

/// The tmux configuration file's name.
const TMUX_CONF_FILE: &str = "tmux.conf";

/// The directory a newer release's asset is downloaded into.
const UPDATES_DIR: &str = "updates";

/// Owner-only: the data directory holds the hook secret (in the hook state
/// file and in every session settings file).
const DATA_DIR_MODE: u32 = 0o700;

/// The paths Delta writes, all under one data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLayout {
    dir: PathBuf,
}

impl DataLayout {
    /// The layout rooted at `data_dir`.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: data_dir.into(),
        }
    }

    /// The data directory itself.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The SQLite overlay. Its `-wal`/`-shm` and `.bak-v<N>` siblings land
    /// beside it.
    pub fn database(&self) -> PathBuf {
        self.dir.join(DATABASE_FILE)
    }

    /// The hook state file, which keeps the hook secret and the desktop app's
    /// port across restarts.
    pub fn hook_state(&self) -> PathBuf {
        self.dir.join(HOOK_STATE_FILE)
    }

    /// The base of the per-spawn working directories (`sessions/<token>`).
    pub fn sessions(&self) -> PathBuf {
        self.dir.join(SESSIONS_DIR)
    }

    /// The Claude Code session settings file for a server on `port`, handed to
    /// `claude --settings <path>`.
    ///
    /// Namespaced by port so two servers on different ports — whose hook URLs
    /// differ — never share one file. Outside every session's working
    /// directory, so a launch in a real repository never overwrites that
    /// repository's own `.claude/settings.json`.
    pub fn session_settings(&self, port: u16) -> PathBuf {
        self.settings_dir().join(format!("{port}.json"))
    }

    /// The directory holding one session settings file per port.
    pub fn settings_dir(&self) -> PathBuf {
        self.dir.join(SETTINGS_DIR)
    }

    /// The configuration file Delta's tmux server is started with (`tmux -f`).
    pub fn tmux_conf(&self) -> PathBuf {
        self.dir.join(TMUX_CONF_FILE)
    }

    /// The directory the desktop app downloads a newer release's asset into.
    /// Created by the download, not at startup: only a desktop release build
    /// ever downloads. A file under its asset's name is always a verified one;
    /// one being downloaded carries a `.part` suffix.
    pub fn updates(&self) -> PathBuf {
        self.dir.join(UPDATES_DIR)
    }

    /// Create the data directory (owner-only) and `sessions/` in it, before
    /// anything is opened or written there.
    ///
    /// A directory that already exists is left as it is, mode included: an
    /// explicit `DELTA_DATA_DIR` may name a directory the user owns and shares
    /// on purpose. The files carrying the hook secret are owner-only on their
    /// own (the hook state file, and `settings/`, which the workspace gateway
    /// creates owner-only).
    pub fn create_dirs(&self) -> Result<(), DataDirError> {
        for dir in [self.dir.clone(), self.sessions()] {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(DATA_DIR_MODE)
                .create(&dir)
                .map_err(|source| DataDirError { path: dir, source })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    #[test]
    fn create_dirs_makes_an_owner_only_data_dir_with_its_sessions_dir() {
        let root = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(root.path().join("nested").join("data"));

        layout.create_dirs().unwrap();
        // Idempotent: a second start finds both in place.
        layout.create_dirs().unwrap();

        for dir in [layout.dir().to_path_buf(), layout.sessions()] {
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, DATA_DIR_MODE, "{}", dir.display());
        }
    }
}
