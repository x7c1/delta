//! Runtime configuration for the composition root.

use crate::settings::render_session_settings;

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
    /// Base directory for per-session git worktrees
    /// (`<base>/<org>-<repo>-<session-id>`, where `<org>-<repo>` is the
    /// repository-identity slug; an origin-less local clone falls back to
    /// `<base>/<repo>-<session-id>`).
    ///
    /// Deliberately a *neutral* location outside any repository tree (default
    /// `$HOME/.delta/worktrees`), not under [`Self::session_workdir_base`]:
    /// Claude Code walks up from its cwd discovering `CLAUDE.md` and
    /// `.claude/settings.json`, so a worktree nested inside another repo would
    /// inherit that repo's `CLAUDE.md` (a blocking external-import prompt) and
    /// its settings/hooks. Placing worktrees here keeps each one isolated.
    pub worktree_base: String,
    /// The dedicated tmux socket Delta's sessions live on (`tmux -L <socket>`).
    pub tmux_socket: String,
    /// The per-run bearer token the API and live sockets require, enforced by
    /// the server's auth guard. Minted (or handed in) once for the server's
    /// lifetime by `delta_server::config::config_from_env`; the frontend
    /// presents it on every request. Not a wire field and never rotated — see
    /// the auth guard.
    pub auth_token: String,
    /// The per-run hook secret carried back on every hook URL as `?hs=<secret>`,
    /// enforced by the server's hook auth guard. Minted once for the server's
    /// lifetime by `delta_server::config::config_from_env` and rendered into
    /// the session settings by [`render_session_settings`] so genuine Claude
    /// Code callbacks present it and a forged local POST cannot. Not a wire
    /// field.
    pub hook_secret: String,
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
    /// How sessions are launched (which binary) and how long the launch
    /// watchdog waits. Defaults are production values; tests and alternative
    /// installs override the binary and shrink the deadlines.
    pub launch: delta_usecase::LaunchConfig,
}

impl Config {
    /// The Claude Code session settings JSON rendered for this configuration, so
    /// the hook URLs always match the running port.
    pub fn session_settings_json(&self) -> String {
        render_session_settings(self.port, &self.hook_secret)
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
