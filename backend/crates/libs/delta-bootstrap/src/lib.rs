//! Composition root.
//!
//! [`build`] constructs the concrete gateways — [`SqliteStore`],
//! [`JsonlTranscript`], [`Tmux`], [`FsWorkspace`] — injects them into an
//! [`Interactor`], and returns the wired application state for the server to
//! drive. This crate is the single place that knows about every concrete
//! implementation; the server depends only on the resulting [`AppInteractor`]
//! alias and the use-case API.

mod error;
mod settings;

pub use error::{Error, Result};
pub use settings::render_session_settings;

// Re-exported so a binary that only configures the server (a test harness,
// the `delta-server` main) can name the launch settings without depending on
// the use-case crate directly.
pub use delta_usecase::LaunchConfig;

use delta_sqlite::SqliteStore;
use delta_transcript::JsonlTranscript;
use delta_usecase::{BoxedInteractor, Interactor};
use git_worktree::Git;
use tmux_driver::Tmux;
use workspace_fs::FsWorkspace;

/// The fully-wired Interactor.
///
/// The gateways are type-erased behind trait objects so the transport layer's
/// shared state is a single non-generic type, shared between this production
/// wiring and the integration tests that substitute fakes.
pub type AppInteractor = BoxedInteractor;

/// Default name of Delta's dedicated tmux socket (`tmux -L <socket>`).
///
/// Delta runs its sessions on their own tmux server so they are isolated from
/// the user's default tmux server — no clutter in the user's `tmux ls`, and the
/// server starts with Delta's own fixed config (via `tmux -f`) instead of the
/// user's `~/.tmux.conf`, so the embedded pane is identical on every machine.
pub const DEFAULT_TMUX_SOCKET: &str = "delta";

/// Runtime configuration for the composition root.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to the SQLite database file holding the thread overlay.
    pub database_path: String,
    /// Base directory for per-spawn working directories. Each spawned session
    /// runs in its own `<base>/<token>` subdirectory, so the `cwd ↔ spawn`
    /// mapping is 1:1 and the hook-binding correlation is exact.
    pub session_workdir_base: String,
    /// The dedicated tmux socket Delta's sessions live on (`tmux -L <socket>`).
    pub tmux_socket: String,
    /// TCP port the server listens on, used to render the session's hook URLs.
    pub port: u16,
    /// How sessions are launched (which binary) and how long the launch
    /// watchdog waits. Defaults are production values; tests and alternative
    /// installs override the binary and shrink the deadlines.
    pub launch: delta_usecase::LaunchConfig,
}

impl Config {
    /// The Claude Code session settings JSON rendered for this configuration, so
    /// the hook URLs always match the running port.
    pub fn session_settings_json(&self) -> String {
        render_session_settings(self.port)
    }

    /// Delta-owned path the rendered settings JSON is written to and handed to
    /// `claude --settings <path>`.
    ///
    /// Lives under the system temp directory (never a user project), so spawning
    /// or resuming in a real repository never overwrites that repository's own
    /// `.claude/settings.json`. Namespaced by port so two Delta servers on
    /// different ports — whose hook URLs differ — never share one file.
    pub fn session_settings_path(&self) -> String {
        std::env::temp_dir()
            .join(format!("delta-{}", self.port))
            .join("settings.json")
            .to_string_lossy()
            .into_owned()
    }
}

/// Construct the wired [`AppInteractor`] from configuration.
///
/// Opening the store applies the schema migration. The transcript path is not
/// needed here — it is learned from the first `UserPromptSubmit` hook. The
/// stateless [`Tmux`] driver mints a fresh tmux session per spawn, so no fixed
/// session name is configured.
pub fn build(config: &Config) -> Result<AppInteractor> {
    let store = SqliteStore::open(&config.database_path)?;
    let transcript = JsonlTranscript::new();
    let tmux = Tmux::new(config.tmux_socket.clone());
    let workspace = FsWorkspace::new();
    let git_worktree = Git::new();
    Ok(Interactor::new(
        Box::new(tmux) as Box<dyn delta_usecase::TmuxDriver>,
        Box::new(transcript) as Box<dyn delta_usecase::Transcript>,
        Box::new(store) as Box<dyn delta_usecase::SessionStore>,
        Box::new(workspace) as Box<dyn delta_usecase::Workspace>,
        Box::new(git_worktree) as Box<dyn delta_usecase::GitWorktree>,
        config.session_workdir_base.clone(),
        config.session_settings_json(),
        config.session_settings_path(),
    )
    .with_launch_config(config.launch.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            database_path: ":memory:".into(),
            session_workdir_base: "/tmp/delta-session".into(),
            tmux_socket: DEFAULT_TMUX_SOCKET.into(),
            port: 7878,
            launch: delta_usecase::LaunchConfig::default(),
        }
    }

    #[test]
    fn build_wires_an_interactor_with_in_memory_store() {
        assert!(build(&test_config()).is_ok());
    }
}
