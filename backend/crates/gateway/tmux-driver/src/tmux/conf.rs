//! Delta's fixed tmux configuration and the hardened write that renders it.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tokio::io::AsyncWriteExt;

use super::Tmux;
use crate::error::Error;

/// Permission bits for the rendered [`DELTA_TMUX_CONF`] file: owner read/write,
/// nobody else.
const CONF_FILE_MODE: u32 = 0o600;

/// Delta's fixed tmux configuration, loaded via `-f` when Delta's server starts.
///
/// Starting the server with `-f <file>` makes tmux skip the user's
/// `~/.tmux.conf` (and the system config) entirely, so the embedded pane behaves
/// identically on every machine no matter how the user has themed or rebound
/// their own tmux. This config is the *only* customization Delta applies, and
/// every line is a deliberate requirement of the embedded pane — not a style
/// preference. See [`Tmux::output`] (which passes `-f`) and
/// [`create_session`](delta_usecase::TmuxDriver::create_session) (which writes
/// this file before the server-starting `new-session`).
pub(super) const DELTA_TMUX_CONF: &str = "\
# Delta's fixed tmux configuration. Delta starts its own tmux server with
# `-f <this file>`, which makes tmux ignore the user's ~/.tmux.conf so the
# embedded pane is identical on every machine. Every line below is a deliberate
# requirement of the embedded pane, not a preference.

# Pin the terminal type the Claude pane runs under so its terminfo (and the
# capabilities the TUI probes) are the same everywhere. screen-256color is
# preferred over tmux's own default (tmux-256color) because its terminfo entry
# is present on far more machines out of the box.
set -g default-terminal \"screen-256color\"

# Vanilla tmux holds a lone ESC from a client for 500ms (escape-time) to see
# whether it is the start of an escape sequence. Delta's only attach clients
# are PTY-bridged xterm.js terminals, whose escape sequences always arrive in
# one complete write, so the disambiguation wait buys nothing — it only delays
# Escape (the interrupt key for the Claude TUI) by half a second. Deliver it
# immediately.
set -s escape-time 0

# focus-events is off in vanilla tmux but a common user override turns it on.
# With it on, tmux reports focus in/out to the pane program every time a client
# attaches/detaches (which the embedded terminal does on every session switch),
# and Claude's TUI renders each report as a stray blank line. Pin it off.
set -s focus-events off

# Vanilla tmux shows a status bar; Delta's pane is a permission-answering escape
# hatch, not a full tmux workspace, so the bar only wastes a row and renders the
# user's themed powerline/Nerd-Font glyphs as tofu in the browser xterm.
set -g status off

# Deepen the scrollback so the embedded terminal can scroll far enough back
# through Claude's output to be useful for debugging (via copy-mode: prefix `[`).
# Vanilla tmux keeps only 2000 lines, which a single verbose tool output can
# fill; 10000 lines costs only a few MB per pane.
set -g history-limit 10000
";

impl Tmux {
    /// Write [`DELTA_TMUX_CONF`] to [`Tmux::conf_path`], owner-readable only and
    /// never through a symlink.
    ///
    /// The path is fully predictable (the production socket is a constant), and
    /// tmux *executes* every directive in the file it is handed via `-f`. On a
    /// shared Linux host, where `/tmp` is world-writable, another local user
    /// could therefore pre-plant this file — or a symlink standing in for it —
    /// and have Delta's tmux server load their directives. (macOS `$TMPDIR` is
    /// already per-user 0700, so the exposure is the multi-user Linux case.)
    /// `O_NOFOLLOW` makes the `open(2)` fail on a symlink instead of following
    /// it, and 0600 keeps the file un-writable by anyone but its owner
    /// afterwards; `mode` applies only on creation, so a file left at 0644 by an
    /// older Delta run is tightened explicitly.
    pub(super) async fn write_conf(&self) -> std::result::Result<(), Error> {
        let path = Path::new(&self.conf_path);
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(CONF_FILE_MODE)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .await
            .map_err(|source| self.conf_error(source))?;
        file.set_permissions(std::fs::Permissions::from_mode(CONF_FILE_MODE))
            .await
            .map_err(|source| self.conf_error(source))?;
        file.write_all(DELTA_TMUX_CONF.as_bytes())
            .await
            .map_err(|source| self.conf_error(source))?;
        file.flush()
            .await
            .map_err(|source| self.conf_error(source))?;
        Ok(())
    }

    /// Wrap an I/O failure against [`Tmux::conf_path`] in the config error that
    /// names the path.
    fn conf_error(&self, source: std::io::Error) -> Error {
        Error::Config {
            path: self.conf_path.clone(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A driver whose config path points at `conf_path` instead of the system
    /// temp directory, so the write can be inspected in a test-owned directory.
    fn driver_writing_to(conf_path: &Path) -> Tmux {
        Tmux {
            socket: "delta-test".to_owned(),
            conf_path: conf_path.to_string_lossy().into_owned(),
        }
    }

    #[tokio::test]
    async fn writes_the_conf_with_owner_only_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let conf_path = dir.path().join("delta-tmux-delta.conf");
        // tmux executes every directive in this file, so on a shared host it
        // must not be writable (or even readable) by anyone but its owner.
        driver_writing_to(&conf_path).write_conf().await.unwrap();

        assert_eq!(
            tokio::fs::read_to_string(&conf_path).await.unwrap(),
            DELTA_TMUX_CONF
        );
        let mode = tokio::fs::metadata(&conf_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, CONF_FILE_MODE);
    }

    #[tokio::test]
    async fn rewrites_a_leftover_conf_as_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let conf_path = dir.path().join("delta-tmux-delta.conf");
        // A file left behind by an older Delta run at the default 0644: the
        // creation mode does not apply to it, so the write must tighten it.
        tokio::fs::write(&conf_path, "stale").await.unwrap();
        tokio::fs::set_permissions(&conf_path, std::fs::Permissions::from_mode(0o644))
            .await
            .unwrap();

        driver_writing_to(&conf_path).write_conf().await.unwrap();

        let mode = tokio::fs::metadata(&conf_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, CONF_FILE_MODE);
    }

    #[tokio::test]
    async fn refuses_to_write_the_conf_through_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        // A symlink pre-planted at the (fully predictable) config path: writing
        // through it would let another local user pick the file tmux loads.
        let target = dir.path().join("attacker-owned.conf");
        tokio::fs::write(&target, "untouched").await.unwrap();
        let conf_path = dir.path().join("delta-tmux-delta.conf");
        std::os::unix::fs::symlink(&target, &conf_path).unwrap();

        let err = driver_writing_to(&conf_path)
            .write_conf()
            .await
            .unwrap_err();

        assert!(
            matches!(err, Error::Config { .. }),
            "a symlinked config path is a config write failure, got {err:?}"
        );
        assert_eq!(
            tokio::fs::read_to_string(&target).await.unwrap(),
            "untouched",
            "the link target must not be written or truncated"
        );
    }

    #[test]
    fn fixed_config_pins_the_deliberate_settings() {
        // The whole point of the `-f` config is a host-independent baseline:
        // these lines are the only customization Delta applies, so guard against
        // an edit silently dropping one. (`screen-256color` is pinned over
        // tmux's own default for terminfo portability.)
        assert!(DELTA_TMUX_CONF.contains("set -g default-terminal \"screen-256color\""));
        assert!(DELTA_TMUX_CONF.contains("set -s escape-time 0"));
        assert!(DELTA_TMUX_CONF.contains("set -s focus-events off"));
        assert!(DELTA_TMUX_CONF.contains("set -g status off"));
        assert!(DELTA_TMUX_CONF.contains("set -g history-limit 10000"));
    }
}
