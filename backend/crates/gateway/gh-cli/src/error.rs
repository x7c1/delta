//! Errors raised by the gh CLI gateway.

use thiserror::Error;

/// Result alias used inside this crate; the [`GhCli`] port itself returns
/// [`delta_usecase::Result`].
///
/// [`GhCli`]: delta_usecase::GhCli
pub type Result<T> = std::result::Result<T, Error>;

/// The failure modes a `gh` call can surface to the use case.
///
/// Missing binary / unauthenticated gh are *not* errors here — they are
/// reported through [`delta_usecase::GhCli::is_authenticated`] as
/// `false`, so the use case can degrade gracefully. Errors here are
/// genuine failures (a JSON parse error, an I/O error spawning the
/// process, or a `gh` call exiting non-zero despite a successful auth
/// check) that should bubble up rather than be silently swallowed.
#[derive(Debug, Error)]
pub enum Error {
    /// Spawning `gh` itself failed for a reason other than "binary not
    /// found" (which collapses to an unauthenticated `is_authenticated`
    /// instead).
    #[error("spawn gh: {0}")]
    Io(#[from] std::io::Error),
    /// A `gh` call — the PR search (`gh api graphql`) or a clone
    /// (`gh repo clone`) — exited non-zero. Its stderr is surfaced so a
    /// rate-limit / auth-rotated / unreachable-host failure is
    /// debuggable from logs.
    #[error("gh {command}: {status}: {stderr}")]
    Command {
        command: String,
        status: String,
        stderr: String,
    },
    /// The PR-search call returned output that was not the expected
    /// shape (missing field, wrong type, or invalid JSON).
    #[error("parse gh output: {0}")]
    Parse(String),
}

impl From<Error> for delta_usecase::Error {
    fn from(err: Error) -> Self {
        delta_usecase::Error::Gh(err.to_string())
    }
}
