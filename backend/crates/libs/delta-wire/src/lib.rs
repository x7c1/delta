//! Composition root.
//!
//! [`build`] constructs the concrete gateways — [`SqliteStore`],
//! [`JsonlTranscript`], [`Tmux`] — injects them into an [`Interactor`], and
//! returns the wired application state for the server to drive. This crate is
//! the single place that knows about every concrete implementation; the server
//! depends only on the resulting [`AppInteractor`] alias and the use-case API.

mod error;

pub use error::{Error, Result};

use delta_sqlite::SqliteStore;
use delta_transcript::JsonlTranscript;
use delta_usecase::{BoxedInteractor, Interactor};
use tmux_driver::Tmux;

/// The fully-wired Interactor.
///
/// The gateways are type-erased behind trait objects so the transport layer's
/// shared state is a single non-generic type, shared between this production
/// wiring and the integration tests that substitute fakes.
pub type AppInteractor = BoxedInteractor;

/// Runtime configuration for the composition root.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to the SQLite database file holding the thread overlay.
    pub database_path: String,
    /// The tmux target pane that hosts the Claude Code session.
    pub tmux_pane: String,
}

/// Construct the wired [`AppInteractor`] from configuration.
///
/// Opening the store applies the schema migration. The transcript path is not
/// needed here — it is learned from the first `UserPromptSubmit` hook.
pub fn build(config: &Config) -> Result<AppInteractor> {
    let store = SqliteStore::open(&config.database_path)?;
    let transcript = JsonlTranscript::new();
    let tmux = Tmux::new(config.tmux_pane.clone());
    Ok(Interactor::new(
        Box::new(tmux) as Box<dyn delta_usecase::TmuxDriver>,
        Box::new(transcript) as Box<dyn delta_usecase::Transcript>,
        Box::new(store) as Box<dyn delta_usecase::SessionStore>,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_wires_an_interactor_with_in_memory_store() {
        let interactor = build(&Config {
            database_path: ":memory:".into(),
            tmux_pane: "delta:0.0".into(),
        });
        assert!(interactor.is_ok());
    }
}
