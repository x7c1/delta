//! Subprocess-backed [`ExternalOpener`] for launching editors and IDEs.
//!
//! [`SystemOpener`] spawns the target command with the given arguments as a
//! *detached* child (the parent does not wait for it to exit) using
//! [`tokio::process::Command`]. It never invokes a shell: the command name
//! and each argument are passed verbatim, so no metacharacter in a
//! user-supplied path is ever interpreted.
//!
//! A missing binary on `PATH` maps to
//! [`delta_usecase::Error::ExternalOpenerCommandNotFound`] so the browser
//! can render a specific "the tool is not installed" message rather than a
//! generic failure. Any other spawn failure — fork error, permission
//! denied — maps to [`delta_usecase::Error::ExternalOpenerSpawnFailed`].

use std::io::ErrorKind;

use async_trait::async_trait;
use tokio::process::Command;

use delta_usecase::{Error, ExternalOpener, Result};

/// The default [`ExternalOpener`] implementation, backed by
/// [`tokio::process::Command`].
///
/// Stateless and cheap to clone; a single instance can serve every request
/// on the server.
#[derive(Debug, Default, Clone)]
pub struct SystemOpener;

impl SystemOpener {
    /// Construct a new [`SystemOpener`].
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ExternalOpener for SystemOpener {
    async fn open(&self, command: &str, args: Vec<String>) -> Result<()> {
        // Never wait for the child: the editor's lifetime is independent of
        // the browser click that launched it. `.spawn()` returns immediately
        // once the fork/exec has taken (or has irrecoverably failed);
        // dropping the returned `Child` detaches the process — we do not
        // want to inherit its zombie either, so `kill_on_drop(false)` is the
        // default and stays that way.
        let mut cmd = Command::new(command);
        cmd.args(&args);
        // Suppress the child's stdio: it inherits from the server otherwise,
        // and a chatty editor would end up scribbling into the server's
        // stdout/stderr. This is a fire-and-forget spawn — nothing in the
        // parent needs its output.
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        match cmd.spawn() {
            Ok(child) => {
                // Detach: we do not want the async runtime to reap the child
                // (it would block waiting for it), but we also do not want a
                // zombie. Dropping the handle lets the OS handle it.
                drop(child);
                tracing::info!(
                    command = command,
                    args = ?args,
                    "external opener: spawned"
                );
                Ok(())
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Err(
                Error::ExternalOpenerCommandNotFound(format!("{command}: {err}")),
            ),
            Err(err) => Err(Error::ExternalOpenerSpawnFailed(format!(
                "{command}: {err}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missing binary maps to `ExternalOpenerCommandNotFound` with a
    /// message that names the command.
    #[tokio::test]
    async fn missing_binary_reports_command_not_found() {
        let opener = SystemOpener::new();
        // A binary name that cannot exist on any host — extra-uncommon prefix
        // so no test host has a real installation to collide with.
        let err = opener
            .open(
                "delta-nonexistent-tool-xyz",
                vec!["/tmp/anywhere".to_owned()],
            )
            .await
            .expect_err("the binary does not exist, so the spawn must fail");

        match err {
            Error::ExternalOpenerCommandNotFound(msg) => {
                assert!(
                    msg.contains("delta-nonexistent-tool-xyz"),
                    "the message must name the missing command, got: {msg}"
                );
            }
            other => panic!("expected ExternalOpenerCommandNotFound, got {other:?}"),
        }
    }

    /// A binary that exists spawns without waiting on the child.
    #[tokio::test]
    async fn true_binary_spawns_and_returns_ok() {
        // `true` is universally available on both Linux and macOS and exits
        // instantly, so this is a robust smoke test for the happy path.
        let opener = SystemOpener::new();
        opener
            .open("true", vec![])
            .await
            .expect("`true` is available on every supported platform");
    }
}
