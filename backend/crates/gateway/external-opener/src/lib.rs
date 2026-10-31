//! Subprocess-backed [`ExternalOpener`] for launching editors and IDEs.
//!
//! [`SystemOpener`] spawns the target command with the given arguments as a
//! *detached* child (the parent does not wait for it to exit) using
//! [`tokio::process::Command`]. It never invokes a shell: the command name
//! and each argument are passed verbatim, so no metacharacter in a
//! user-supplied path is ever interpreted. Keeping the path from being read
//! as one of the command's own options is the caller's part: the interactor
//! passes `--` before it.
//!
//! A missing binary on `PATH` maps to
//! [`delta_usecase::Error::ExternalOpenerCommandNotFound`] so the browser
//! can render a specific "the tool is not installed" message rather than a
//! generic failure. Any other spawn failure — fork error, permission
//! denied — maps to [`delta_usecase::Error::ExternalOpenerSpawnFailed`].
//!
//! Each spawn also asks an [`ActivationTokenSource`] for an activation token
//! and, when it gets one, sets it on the child as `XDG_ACTIVATION_TOKEN` and
//! `DESKTOP_STARTUP_ID` ([`ACTIVATION_TOKEN_VARS`]), the pair a desktop
//! launcher sets. A Wayland compositor lets the opened app's window come to
//! the front only with such a token; without one GNOME shows an "is ready"
//! notification instead. Only the desktop shell can mint one (it owns the
//! focused window the token is granted for), so it supplies the source when it
//! builds the server; everywhere else the source is [`NoActivationToken`] and
//! the child is started without the variables, as before. Activation is a
//! concern of the spawn, not of the use case, so the [`ExternalOpener`] port
//! knows nothing of it.

use std::fmt::Debug;
use std::io::ErrorKind;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::process::Command;

use delta_usecase::{Error, ExternalOpener, Result};

/// The variables an activation token is handed to a launched app in: the
/// xdg-activation one Wayland reads, and the startup-notification ID X11 (and
/// GTK 3) reads. A launcher sets both to the same token.
pub const ACTIVATION_TOKEN_VARS: [&str; 2] = ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"];

/// Where [`SystemOpener`] gets the activation token for each command it opens.
///
/// A token is single-use, so one is asked for per spawn, right before it.
/// `None` means the command is opened without one. An implementation must not
/// hold the spawn up for long: a launch is never refused, nor delayed beyond a
/// short wait, for want of a token.
#[async_trait]
pub trait ActivationTokenSource: Debug + Send + Sync {
    /// A fresh activation token for launching `command`, or `None` when there
    /// is none to be had.
    async fn activation_token(&self, command: &str) -> Option<String>;
}

/// The [`ActivationTokenSource`] of a server with no window of its own to mint
/// a token for — the CLI server, `make dev`, the tests: it never has one.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoActivationToken;

impl NoActivationToken {
    /// The source as the shared handle [`SystemOpener::new`] takes.
    pub fn arc() -> Arc<dyn ActivationTokenSource> {
        Arc::new(Self)
    }
}

#[async_trait]
impl ActivationTokenSource for NoActivationToken {
    async fn activation_token(&self, _command: &str) -> Option<String> {
        None
    }
}

/// The default [`ExternalOpener`] implementation, backed by
/// [`tokio::process::Command`].
///
/// Cheap to clone; a single instance can serve every request on the server.
#[derive(Debug, Clone)]
pub struct SystemOpener {
    /// Environment variables set on every command it opens, on top of the
    /// inherited environment (the server configuration's `child_env`). A bare
    /// command name is also looked up on this `PATH` when it carries one.
    env: Vec<(String, String)>,
    /// Where the activation token handed to each opened command comes from.
    activation: Arc<dyn ActivationTokenSource>,
}

impl SystemOpener {
    /// Construct a new [`SystemOpener`] that opens every command with `env`
    /// set, and with the activation token `activation` gives for it when it
    /// gives one.
    pub fn new(env: Vec<(String, String)>, activation: Arc<dyn ActivationTokenSource>) -> Self {
        Self { env, activation }
    }

    /// The command `<command> <args>`, with [`Self::env`] set, the activation
    /// `token` set in [`ACTIVATION_TOKEN_VARS`] when there is one, and its
    /// stdio suppressed.
    fn command(&self, command: &str, args: &[String], token: Option<&str>) -> Command {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .envs(self.env.iter().map(|(name, value)| (name, value)));
        if let Some(token) = token {
            cmd.envs(ACTIVATION_TOKEN_VARS.map(|name| (name, token)));
        }
        // Suppress the child's stdio: it inherits from the server otherwise,
        // and a chatty editor would end up scribbling into the server's
        // stdout/stderr. This is a fire-and-forget spawn — nothing in the
        // parent needs its output.
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd
    }
}

#[async_trait]
impl ExternalOpener for SystemOpener {
    async fn open(&self, command: &str, args: Vec<String>) -> Result<()> {
        let token = self.activation.activation_token(command).await;
        // Never wait for the child: the editor's lifetime is independent of
        // the browser click that launched it. `.spawn()` returns immediately
        // once the fork/exec has taken (or has irrecoverably failed);
        // dropping the returned `Child` detaches the process — we do not
        // want to inherit its zombie either, so `kill_on_drop(false)` is the
        // default and stays that way.
        match self.command(command, &args, token.as_deref()).spawn() {
            Ok(child) => {
                // Detach: we do not want the async runtime to reap the child
                // (it would block waiting for it), but we also do not want a
                // zombie. Dropping the handle lets the OS handle it.
                drop(child);
                tracing::info!(
                    command = command,
                    args = ?args,
                    activation_token = token.is_some(),
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

    /// A source that always gives the same token.
    #[derive(Debug)]
    struct FixedToken(&'static str);

    #[async_trait]
    impl ActivationTokenSource for FixedToken {
        async fn activation_token(&self, _command: &str) -> Option<String> {
            Some(self.0.to_owned())
        }
    }

    /// An opener with the default source, which gives no token.
    fn opener(env: Vec<(String, String)>) -> SystemOpener {
        SystemOpener::new(env, NoActivationToken::arc())
    }

    /// The variables set on the command `opener` opens `code /work` as, with
    /// the token its source gives, sorted by name.
    async fn opened_env(opener: &SystemOpener) -> Vec<(String, Option<String>)> {
        let token = opener.activation.activation_token("code").await;
        let command = opener.command("code", &["/work".to_owned()], token.as_deref());
        let mut envs: Vec<_> = command
            .as_std()
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        envs.sort();
        envs
    }

    #[tokio::test]
    async fn the_opened_command_carries_the_activation_token_the_source_gives() {
        let opener = SystemOpener::new(Vec::new(), Arc::new(FixedToken("token-1")));

        assert_eq!(
            opened_env(&opener).await,
            [
                ("DESKTOP_STARTUP_ID".to_owned(), Some("token-1".to_owned())),
                (
                    "XDG_ACTIVATION_TOKEN".to_owned(),
                    Some("token-1".to_owned())
                ),
            ]
        );
    }

    /// The default source, which every server without the desktop shell runs
    /// with, gives none: the command carries neither variable.
    #[tokio::test]
    async fn without_a_token_the_opened_command_carries_no_activation_variables() {
        let opener = opener(vec![("LANG".to_owned(), "C".to_owned())]);

        assert_eq!(
            opened_env(&opener).await,
            [("LANG".to_owned(), Some("C".to_owned()))]
        );
    }

    /// A missing binary maps to `ExternalOpenerCommandNotFound` with a
    /// message that names the command.
    #[tokio::test]
    async fn missing_binary_reports_command_not_found() {
        let opener = opener(Vec::new());
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

    #[test]
    fn the_opened_command_carries_the_given_environment() {
        let opener = opener(vec![("LANG".to_owned(), "en_US.UTF-8".to_owned())]);

        let command = opener.command("code", &["/work".to_owned()], None);

        let envs: Vec<_> = command.as_std().get_envs().collect();
        assert_eq!(envs, [("LANG".as_ref(), Some("en_US.UTF-8".as_ref()))]);
    }

    /// The arguments reach the command verbatim and in order, so the `--` the
    /// interactor puts before a path still ends the command's option parsing.
    #[test]
    fn the_opened_command_carries_its_arguments_verbatim() {
        let opener = opener(Vec::new());

        let command = opener.command("code", &["--".to_owned(), "/work dir".to_owned()], None);

        let args: Vec<_> = command.as_std().get_args().collect();
        assert_eq!(args, ["--", "/work dir"]);
    }

    /// A binary that exists spawns without waiting on the child.
    #[tokio::test]
    async fn true_binary_spawns_and_returns_ok() {
        // `true` is universally available on both Linux and macOS and exits
        // instantly, so this is a robust smoke test for the happy path.
        let opener = opener(Vec::new());
        opener
            .open("true", vec![])
            .await
            .expect("`true` is available on every supported platform");
    }
}
