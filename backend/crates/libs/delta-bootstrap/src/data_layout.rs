//! [`DataLayout`]: every path Delta writes, derived from its data directory.
//!
//! The server owns one **data directory** (`Config::data_dir`) and derives
//! every file and directory it writes from it, here and nowhere else:
//!
//! ```text
//! <data_dir>/
//!   auth-token                the running CLI server's bearer token (0600,
//!                             removed on a clean stop)
//!   delta.db                  the SQLite overlay (SQLite adds `-wal`/`-shm`
//!                             beside it, the migration runner `.bak-v<N>`)
//!   delta-hook-state.json     the hook secret and the desktop app's port
//!   logs/                     the desktop app's log files, on Linux only
//!                             (written by the shell, not the server)
//!   sessions/                 left by older versions (per-spawn working
//!                             directories); no longer created
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

use std::io;
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use crate::DataDirError;

/// The bearer token file's name.
const AUTH_TOKEN_FILE: &str = "auth-token";

/// The SQLite overlay's file name.
const DATABASE_FILE: &str = "delta.db";

/// The hook state file's name.
const HOOK_STATE_FILE: &str = "delta-hook-state.json";

/// The name of the per-spawn working-directory base older versions created.
const SESSIONS_DIR: &str = "sessions";

/// The directory holding one session settings file per port.
const SETTINGS_DIR: &str = "settings";

/// The tmux configuration file's name.
const TMUX_CONF_FILE: &str = "tmux.conf";

/// The directory a newer release's asset is downloaded into.
const UPDATES_DIR: &str = "updates";

/// The desktop app's log directory when it sits in the data directory: Tauri's
/// app log directory is `<platform data dir>/<identifier>/logs` on Linux (on
/// macOS it is `~/Library/Logs/<identifier>`, outside the data directory).
const LOGS_DIR: &str = "logs";

/// Owner-only: the data directory holds the hook secret (in the hook state
/// file and in every session settings file) and the bearer token file.
const DATA_DIR_MODE: u32 = 0o700;

/// Where a data directory's path came from, which decides whether Delta may
/// tighten the mode of one that already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataDirOrigin {
    /// The directory Delta names itself (`<platform data dir>/<identifier>`).
    /// Delta owns it, so an existing one is made owner-only.
    Default,
    /// A directory the user named (`DELTA_DATA_DIR`). It may be one the user
    /// owns and shares on purpose, so an existing one keeps its mode.
    Explicit,
}

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

    /// The file the running `delta-server` keeps its per-run bearer token in,
    /// for `delta-server open-url` and other local clients of the same user.
    /// Written owner-only on start and removed on a clean stop.
    pub fn auth_token(&self) -> PathBuf {
        self.dir.join(AUTH_TOKEN_FILE)
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

    /// The base of the per-spawn working directories older versions launched a
    /// session in when it named no directory (`sessions/<token>`).
    ///
    /// No longer created — every session starts in a directory the user chose
    /// — but one left by an older version is still listed in Settings →
    /// Storage, kept owner-only and removed by an erase.
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

    /// The `curl` configuration a command entry of the session settings for a
    /// server on `port` reads to call `/hooks/<endpoint>`, next to
    /// [`Self::session_settings`].
    ///
    /// The URL it holds carries the hook secret, which must not appear on the
    /// command line (see `render_session_settings`), so it lives in this
    /// owner-only file instead.
    pub fn hook_curl_config(&self, port: u16, endpoint: &str) -> PathBuf {
        self.settings_dir().join(format!("{port}-{endpoint}.curl"))
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

    /// Create the data directory (owner-only), before anything is opened or
    /// written there, and make the directories Delta owns owner-only when they
    /// already exist.
    ///
    /// The data directory holds the hook secret, the bearer token file, the
    /// database (every prompt and reply), its snapshots and, on Linux, the
    /// desktop app's logs. A directory created with the umask's default mode
    /// (an older Delta, or Tauri creating its webview data directory there
    /// first) would leave all of that readable by other local users, so:
    ///
    /// - a missing directory is created `0700`;
    /// - an existing `sessions/` (left by an older version) and `updates/` are
    ///   tightened to `0700`, since Delta created them by name wherever the
    ///   data directory is;
    /// - an existing data directory and its `logs/` are tightened to `0700`
    ///   only for [`DataDirOrigin::Default`]: an explicit `DELTA_DATA_DIR` may
    ///   name a directory the user owns and shares on purpose, and its mode is
    ///   the user's choice. The files carrying the hook secret are owner-only on
    ///   their own (the hook state file, and `settings/`, which the workspace
    ///   gateway creates owner-only), and so is the database.
    ///
    /// Only real directories are tightened: a symlink in their place is the
    /// user's own arrangement and is left as it is, target included. A
    /// directory whose mode cannot be changed (one another user owns) is
    /// logged at `warn` and the start goes on.
    pub fn create_dirs(&self, origin: DataDirOrigin) -> Result<(), DataDirError> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(DATA_DIR_MODE)
            .create(&self.dir)
            .map_err(|source| DataDirError {
                path: self.dir.clone(),
                source,
            })?;
        let mut owned = vec![self.sessions(), self.updates()];
        if origin == DataDirOrigin::Default {
            owned.extend([self.dir.clone(), self.dir.join(LOGS_DIR)]);
        }
        for dir in owned {
            if let Err(err) = make_owner_only(&dir) {
                tracing::warn!(
                    path = %dir.display(),
                    "could not make a directory of Delta's owner-only: {err}"
                );
            }
        }
        Ok(())
    }
}

/// Set `dir`'s mode to [`DATA_DIR_MODE`] when it is a real directory whose mode
/// differs; a missing path or a symlink is left alone.
fn make_owner_only(dir: &Path) -> io::Result<()> {
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if !meta.is_dir() || meta.permissions().mode() & 0o777 == DATA_DIR_MODE {
        return Ok(());
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(DATA_DIR_MODE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(dir: &Path) -> u32 {
        std::fs::metadata(dir).unwrap().permissions().mode() & 0o777
    }

    /// `dir` created with `0755` set explicitly, whatever the umask.
    fn open_dir(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn create_dirs_makes_an_owner_only_data_dir_without_a_sessions_dir() {
        let root = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(root.path().join("nested").join("data"));

        layout.create_dirs(DataDirOrigin::Default).unwrap();
        // Idempotent: a second start finds it in place.
        layout.create_dirs(DataDirOrigin::Default).unwrap();

        assert_eq!(mode(layout.dir()), DATA_DIR_MODE);
        assert!(
            !layout.sessions().exists(),
            "no per-spawn working-directory base is created"
        );
    }

    #[test]
    fn create_dirs_tightens_an_existing_default_data_dir_and_its_own_dirs() {
        let root = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(root.path().join("data"));
        let logs = layout.dir().join(LOGS_DIR);
        for dir in [layout.dir(), &layout.sessions(), &layout.updates(), &logs] {
            open_dir(dir);
        }

        layout.create_dirs(DataDirOrigin::Default).unwrap();

        for dir in [layout.dir(), &layout.sessions(), &layout.updates(), &logs] {
            assert_eq!(mode(dir), DATA_DIR_MODE, "{}", dir.display());
        }
    }

    #[test]
    fn create_dirs_leaves_an_explicit_data_dir_s_own_mode_alone() {
        let root = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(root.path().join("shared"));
        let parent_mode = mode(root.path());
        let logs = layout.dir().join(LOGS_DIR);
        for dir in [layout.dir(), &layout.sessions(), &layout.updates(), &logs] {
            open_dir(dir);
        }

        layout.create_dirs(DataDirOrigin::Explicit).unwrap();

        assert_eq!(
            mode(layout.dir()),
            0o755,
            "the user's directory keeps its mode"
        );
        assert_eq!(mode(&logs), 0o755, "a `logs/` there is not Delta's");
        assert_eq!(mode(root.path()), parent_mode, "nor is its parent");
        for dir in [layout.sessions(), layout.updates()] {
            assert_eq!(mode(&dir), DATA_DIR_MODE, "{}", dir.display());
        }
    }

    #[test]
    fn create_dirs_does_not_follow_a_symlinked_data_dir() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("elsewhere");
        open_dir(&target);
        let link = root.path().join("data");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        DataLayout::new(&link)
            .create_dirs(DataDirOrigin::Default)
            .unwrap();

        assert_eq!(mode(&target), 0o755, "a symlink's target is the user's");
    }
}
