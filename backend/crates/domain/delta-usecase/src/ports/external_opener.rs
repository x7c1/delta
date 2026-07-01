//! Spawning external tools (editors, IDEs, viewers) with a target argument.
//!
//! The `open cwd in an external tool` feature spawns a helper command with a
//! path argument — initially only `code <path>` (Visual Studio Code opens the
//! path in a new window). The [`ExternalOpener`] port wraps that spawn so the
//! interactor can drive it without depending on `std::process` directly, and
//! so unit tests can substitute a fake that records the invocation without
//! actually launching anything.
//!
//! Errors are distinguished so the transport layer can report the useful case
//! (`code` is not installed) separately from a generic spawn failure — the
//! browser wants the user to know they need to install VS Code, not a vague
//! "opening failed".

use async_trait::async_trait;

use crate::error::Result;

/// Spawns an external tool with a fixed argument list.
///
/// The interactor resolves which command to run (from the handler registry)
/// and the exact argument list; this port only has to spawn it. Passing a
/// pre-built argument vector (never a shell string) is deliberate: it avoids
/// any risk of shell metacharacter interpretation from a user-supplied path.
#[async_trait]
pub trait ExternalOpener: Send + Sync {
    /// Spawn `command` with the exact `args`.
    ///
    /// The child is *detached* — the opener does not wait for the editor to
    /// exit; it only reports whether the spawn itself succeeded. This matches
    /// how the user experiences the click: they click, VS Code opens (or an
    /// error surfaces), and the browser is not tied to the editor's lifetime.
    ///
    /// A missing `command` on `PATH` reports as
    /// [`crate::Error::ExternalOpenerCommandNotFound`] (so the browser can
    /// show a specific "VS Code is not installed" message); any other failure
    /// (fork failure, permission denied on the binary, etc.) reports as
    /// [`crate::Error::ExternalOpenerSpawnFailed`].
    async fn open(&self, command: &str, args: Vec<String>) -> Result<()>;
}

#[async_trait]
impl ExternalOpener for Box<dyn ExternalOpener> {
    async fn open(&self, command: &str, args: Vec<String>) -> Result<()> {
        (**self).open(command, args).await
    }
}
