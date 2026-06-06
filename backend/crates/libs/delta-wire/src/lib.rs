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

use delta_sqlite::SqliteStore;
use delta_transcript::JsonlTranscript;
use delta_usecase::{BoxedInteractor, Interactor};
use tmux_driver::Tmux;
use workspace_fs::FsWorkspace;

/// The fully-wired Interactor.
///
/// The gateways are type-erased behind trait objects so the transport layer's
/// shared state is a single non-generic type, shared between this production
/// wiring and the integration tests that substitute fakes.
pub type AppInteractor = BoxedInteractor;

/// The command launched in the tmux session.
pub const SESSION_COMMAND: &str = "claude";

/// Runtime configuration for the composition root.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to the SQLite database file holding the thread overlay.
    pub database_path: String,
    /// The tmux session name that hosts the Claude Code session. The pane the
    /// server drives is derived as `<tmux_session>:0.0`.
    pub tmux_session: String,
    /// Working directory the Claude Code session runs in.
    pub session_workdir: String,
    /// TCP port the server listens on, used to render the session's hook URLs.
    pub port: u16,
}

impl Config {
    /// The tmux pane the server drives and bridges the PTY to: `<session>:0.0`.
    pub fn tmux_pane(&self) -> String {
        Tmux::new(self.tmux_session.clone()).target_pane().to_owned()
    }

    /// The Claude Code session settings JSON rendered for this configuration, so
    /// the hook URLs always match the running port.
    pub fn session_settings_json(&self) -> String {
        render_session_settings(self.port)
    }
}

/// Construct the wired [`AppInteractor`] from configuration.
///
/// Opening the store applies the schema migration. The transcript path is not
/// needed here — it is learned from the first `UserPromptSubmit` hook.
pub fn build(config: &Config) -> Result<AppInteractor> {
    let store = SqliteStore::open(&config.database_path)?;
    let transcript = JsonlTranscript::new();
    let tmux = Tmux::new(config.tmux_session.clone());
    let workspace = FsWorkspace::new();
    Ok(Interactor::new(
        Box::new(tmux) as Box<dyn delta_usecase::TmuxDriver>,
        Box::new(transcript) as Box<dyn delta_usecase::Transcript>,
        Box::new(store) as Box<dyn delta_usecase::SessionStore>,
        Box::new(workspace) as Box<dyn delta_usecase::Workspace>,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            database_path: ":memory:".into(),
            tmux_session: "delta".into(),
            session_workdir: "/tmp/delta-session".into(),
            port: 7878,
        }
    }

    #[test]
    fn build_wires_an_interactor_with_in_memory_store() {
        assert!(build(&test_config()).is_ok());
    }

    #[test]
    fn derives_the_pane_from_the_session_name() {
        assert_eq!(test_config().tmux_pane(), "delta:0.0");
    }
}
