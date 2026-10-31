//! Runtime configuration for the composition root.

use std::sync::Arc;

use delta_usecase::{BuildOrigin, HookEndpointChange, Launcher};
use external_opener::ActivationTokenSource;

use crate::data_layout::DataLayout;
use crate::secret::Secret;
use crate::settings::{render_session_settings, SessionSettings};

/// The identifier Delta runs under when nothing names another one.
///
/// It names the data directory (`<platform data dir>/<identifier>`) and,
/// unless `DELTA_TMUX_SOCKET` overrides it, the dedicated tmux socket
/// (`tmux -L <identifier>`). It is the desktop app's bundle identifier, so the
/// installed app, the CLI server and Tauri's own app data directory agree; the
/// desktop dev build (`io.github.x7c1.delta.dev`) and `make dev` run under
/// their own identifier and so never share a data directory or a tmux server
/// with the installed app.
///
/// Delta runs its sessions on their own tmux server so they are isolated from
/// the user's default tmux server — no clutter in the user's `tmux ls`, and the
/// server starts with Delta's own fixed config (via `tmux -f`) instead of the
/// user's `~/.tmux.conf`, so the embedded pane is identical on every machine.
/// A reverse-DNS name rather than a bare word cannot collide with another
/// tool's socket in the user's tmux socket directory.
pub const DEFAULT_IDENTIFIER: &str = "io.github.x7c1.delta";

/// GitHub's endpoint for the newest published release of Delta, which the
/// release check asks unless `DELTA_RELEASE_FEED_URL` overrides it.
pub const DEFAULT_RELEASE_FEED_URL: &str = release_feed::LATEST_RELEASE_URL;

/// Runtime configuration for the composition root.
#[derive(Debug, Clone)]
pub struct Config {
    /// The name this server runs under: it names the default [`Self::data_dir`]
    /// and the default [`Self::tmux_socket`]. See [`DEFAULT_IDENTIFIER`].
    pub identifier: String,
    /// The directory every file Delta writes lives in; [`Self::data_layout`]
    /// derives each path.
    pub data_dir: String,
    /// Base directory for per-session git worktrees
    /// (`<base>/<org>-<repo>-<session-id>`, where `<org>-<repo>` is the
    /// repository-identity slug; an origin-less local clone falls back to
    /// `<base>/<repo>-<session-id>`).
    ///
    /// Deliberately a *neutral* location outside any repository tree (default
    /// `$HOME/.delta/worktrees`), not under [`Self::data_dir`]:
    /// Claude Code walks up from its cwd discovering `CLAUDE.md` and
    /// `.claude/settings.json`, so a worktree nested inside another repo would
    /// inherit that repo's `CLAUDE.md` (a blocking external-import prompt) and
    /// its settings/hooks. Placing worktrees here keeps each one isolated.
    pub worktree_base: String,
    /// The dedicated tmux socket Delta's sessions live on (`tmux -L <socket>`).
    /// Defaults to [`Self::identifier`].
    pub tmux_socket: String,
    /// The per-run bearer token the API and live sockets require, enforced by
    /// the server's auth guard. Minted (or handed in) once for the server's
    /// lifetime by `delta_server::config::config_from_env`. The page trades a
    /// one-time code for it and presents it on every request; the CLI server
    /// also keeps it in [`DataLayout::auth_token`] for local clients. Never
    /// rotated, and never in a page the server serves — see the auth guard.
    pub auth_token: Secret,
    /// The hook secret carried back on every hook URL as `?hs=<secret>`,
    /// enforced by the server's hook auth guard, and rendered into the session
    /// settings by [`render_session_settings`] so genuine Claude Code callbacks
    /// present it and a forged local POST cannot. Not a wire field.
    ///
    /// Unlike [`Self::auth_token`] it outlives the process: the binaries read
    /// it from the hook state file in the data directory (minting and recording
    /// it there when absent) through
    /// `delta_server::config::adopt_persisted_hook_secret`, so a Claude Code
    /// session that survived a restart in tmux still presents the secret the
    /// new process accepts. `DELTA_HOOK_SECRET` overrides it.
    pub hook_secret: Secret,
    /// Whether [`Self::hook_secret`] is the one `DELTA_HOOK_SECRET` named
    /// rather than one minted for want of it, so that adopting the persisted
    /// secret (`delta_server::config::adopt_persisted_hook_secret`) lets it win.
    ///
    /// Read once with the rest of the environment: the variable is then
    /// removed from the environment every child inherits, so it cannot be
    /// read again later. Configuration built by hand (tests) leaves it `false`.
    pub hook_secret_from_env: bool,
    /// The directory a hook-reported `transcript_path` must resolve under to be
    /// persisted and read. Defaults to
    /// `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects` (where real Claude Code
    /// writes transcripts) and is overridable via `DELTA_TRANSCRIPT_ROOT` so the
    /// fake harness — which writes elsewhere — still validates. Threaded into the
    /// [`Interactor`] so `register_session` can reject a transcript path outside
    /// it before it is ever read from disk.
    ///
    /// [`Interactor`]: delta_usecase::Interactor
    pub transcript_root: String,
    /// TCP port the server listens on, used to render the session's hook URLs.
    pub port: u16,
    /// How the hook endpoint this run serves — the port and secret in the
    /// hook URLs — relates to the one the previous run served, as far as the
    /// hook state file can tell. When it differs, a Claude Code session that
    /// survived the restart in tmux, still holding the old URLs, can no longer
    /// reach this server.
    ///
    /// Decided against the hook state file in the data directory (see
    /// `delta_server::config::hook_state`).
    /// [`HookEndpointChange::PortMoved`] when the desktop app could not get its
    /// recorded port back and fell back to a fresh one: the old URLs then point
    /// at a port another process may hold, so re-adoption ends those sessions.
    /// [`HookEndpointChange::Changed`] when the secret was minted afresh (a
    /// first run, an upgrade from a build that did not keep one, or the file
    /// deleted to rotate it) or an explicit `DELTA_HOOK_SECRET` differs from
    /// the recorded one. [`HookEndpointChange::Unchanged`] otherwise, including
    /// the CLI's fixed port, which is never compared. Configuration built by
    /// hand (tests) leaves it `Unchanged`.
    ///
    /// It says whether surviving sessions *would* be stranded, not that any
    /// exist: a first run reads `Changed` with no session to strand, so a
    /// caller pairs it with the sessions it actually finds. Explicit overrides
    /// are blind spots, because neither `DELTA_PORT` nor `DELTA_HOOK_SECRET` is
    /// recorded: an explicit port is never compared, so moving to or from one
    /// reads as no move; and a secret is compared with the recorded one, not
    /// with the previous run's, so the same `DELTA_HOOK_SECRET` on every start
    /// reads `Changed` each time, while dropping it after a run that used it
    /// reads `Unchanged`.
    pub hook_endpoint_change: HookEndpointChange,
    /// How sessions are launched (which binary) and how long the launch
    /// watchdog waits. Defaults are production values; tests and alternative
    /// installs override the binary and shrink the deadlines.
    pub launch: delta_usecase::LaunchConfig,
    /// Environment variables set on every child process Delta spawns — `tmux`
    /// (and through its server, the agent in each pane), `codex`, `git`, `gh`,
    /// the opener and the startup `claude --version` probe — on top of the
    /// environment the server inherited, so these values win over inherited
    /// ones. Bare names are also resolved against this `PATH` when it carries
    /// one, by the spawns and by the binary detector alike.
    ///
    /// Empty by default: the CLI server and `make dev` leave it so, and their
    /// children inherit the terminal's environment. Only the desktop shell
    /// fills it, with the login shell's `PATH` and locale variables (`LANG`,
    /// `LC_*`), because an app launched from Finder or a desktop file inherits
    /// none of them. The set is deliberately that small: variables a user's rc
    /// files export, such as API keys or proxy settings, are not imported
    /// implicitly. A user who wants an agent to see a variable sets it
    /// explicitly, never by accident.
    pub child_env: Vec<(String, String)>,
    /// Where the external opener gets the activation token it hands each app
    /// it starts ("Open in VS Code"), so a Wayland compositor lets that app's
    /// window come to the front.
    ///
    /// [`NoActivationToken`] by default: a server with no window of its own
    /// (the CLI server, `make dev`) has no token to give, and starts the app
    /// without one. Only the desktop shell supplies a source, which mints a
    /// token for its focused window.
    ///
    /// [`NoActivationToken`]: external_opener::NoActivationToken
    pub launch_activation: Arc<dyn ActivationTokenSource>,
    /// Where the release check asks for the newest published release
    /// (`DELTA_RELEASE_FEED_URL`, default [`DEFAULT_RELEASE_FEED_URL`]), or
    /// `None` when the check is turned off: the server then never asks, and
    /// never reports a newer release.
    pub release_feed_url: Option<String>,
    /// Which shell launched the server. Only the desktop shell's
    /// configuration (`delta_server::config::config_from_env_for`) names
    /// [`Launcher::Desktop`]; everything else is the CLI. With
    /// [`Self::build_origin`] it decides whether an update may be downloaded
    /// (see [`delta_usecase::ReleaseUpdate`]).
    pub launcher: Launcher,
    /// Where this build was made, embedded at compile time from
    /// `DELTA_BUILD_ORIGIN` (`delta_server::config`): only a build the release
    /// workflow made may be replaced by a release.
    pub build_origin: BuildOrigin,
}

impl Config {
    /// The paths this configuration writes, all under [`Self::data_dir`].
    pub fn data_layout(&self) -> DataLayout {
        DataLayout::new(&self.data_dir)
    }

    /// The Claude Code session settings rendered for this configuration, so
    /// the hook URLs always match the running port: the JSON written to
    /// [`Self::session_settings_path`], and the `curl` configurations its
    /// command entries read, written next to it.
    pub fn session_settings(&self) -> SessionSettings {
        render_session_settings(
            &self.data_layout(),
            self.port,
            &self.hook_secret,
            self.launch.permission_decision_deadline,
        )
    }

    /// Delta-owned path the rendered settings JSON is written to and handed to
    /// `claude --settings <path>`: [`DataLayout::session_settings`] for this
    /// configuration's port. With the port and hook secret kept across
    /// restarts, a restart lands on the same path and rewrites it with the same
    /// contents.
    pub fn session_settings_path(&self) -> String {
        self.data_layout()
            .session_settings(self.port)
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            identifier: DEFAULT_IDENTIFIER.into(),
            data_dir: "/data/delta".into(),
            worktree_base: "/home/u/.delta/worktrees".into(),
            tmux_socket: DEFAULT_IDENTIFIER.into(),
            auth_token: "tok".into(),
            hook_secret: "hs".into(),
            hook_secret_from_env: false,
            transcript_root: "/home/u/.claude/projects".into(),
            port: 4000,
            hook_endpoint_change: HookEndpointChange::Unchanged,
            launch: delta_usecase::LaunchConfig::default(),
            child_env: Vec::new(),
            launch_activation: external_opener::NoActivationToken::arc(),
            release_feed_url: None,
            launcher: Launcher::Cli,
            build_origin: BuildOrigin::Local,
        }
    }

    #[test]
    fn debug_output_carries_neither_secret() {
        let config = Config {
            auth_token: "the-bearer-token-value".into(),
            hook_secret: "the-hook-secret-value".into(),
            ..config()
        };
        let printed = format!("{config:?}");
        assert!(!printed.contains("the-bearer-token-value"), "{printed}");
        assert!(!printed.contains("the-hook-secret-value"), "{printed}");
        assert!(printed.contains("auth_token: ***"), "{printed}");
    }

    #[test]
    fn every_derived_path_sits_under_the_data_dir() {
        let config = config();
        let layout = config.data_layout();
        let data_dir = std::path::Path::new("/data/delta");

        assert_eq!(layout.dir(), data_dir);
        assert_eq!(layout.database(), data_dir.join("delta.db"));
        assert_eq!(layout.hook_state(), data_dir.join("delta-hook-state.json"));
        assert_eq!(layout.sessions(), data_dir.join("sessions"));
        assert_eq!(
            layout.session_settings(config.port),
            data_dir.join("settings").join("4000.json")
        );
        assert_eq!(layout.tmux_conf(), data_dir.join("tmux.conf"));
        assert_eq!(layout.updates(), data_dir.join("updates"));
        assert_eq!(
            config.session_settings_path(),
            "/data/delta/settings/4000.json"
        );
    }
}
