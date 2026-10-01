//! Runtime configuration for the composition root.

use crate::settings::render_session_settings;

/// Default name of Delta's dedicated tmux socket (`tmux -L <socket>`).
///
/// Delta runs its sessions on their own tmux server so they are isolated from
/// the user's default tmux server — no clutter in the user's `tmux ls`, and the
/// server starts with Delta's own fixed config (via `tmux -f`) instead of the
/// user's `~/.tmux.conf`, so the embedded pane is identical on every machine.
///
/// The name is the app identifier (the same one that names the app data
/// directory) rather than a bare word, so it cannot collide with another
/// tool's socket in the user's tmux socket directory. `scripts/dev.sh` uses
/// `<identifier>.dev` instead, so `make dev` never shares a server with the
/// desktop app.
pub const DEFAULT_TMUX_SOCKET: &str = "io.github.x7c1.delta";

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
    /// The hook secret carried back on every hook URL as `?hs=<secret>`,
    /// enforced by the server's hook auth guard, and rendered into the session
    /// settings by [`render_session_settings`] so genuine Claude Code callbacks
    /// present it and a forged local POST cannot. Not a wire field.
    ///
    /// Unlike [`Self::auth_token`] it outlives the process: the binaries read
    /// it from the hook state file beside the database (minting and recording
    /// it there when absent) through
    /// `delta_server::config::adopt_persisted_hook_secret`, so a Claude Code
    /// session that survived a restart in tmux still presents the secret the
    /// new process accepts. `DELTA_HOOK_SECRET` overrides it.
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
    /// Whether the hook endpoint this run serves — the port and secret in the
    /// hook URLs — differs from the one the previous run served, as far as the
    /// hook state file can tell. When it does, a Claude Code session that
    /// survived the restart in tmux, still holding the old URLs, can no longer
    /// reach this server.
    ///
    /// Decided against the hook state file beside the database (see
    /// `delta_server::config::hook_state`). `true` when the secret was minted
    /// afresh (a first run, an upgrade from a build that did not keep one, or
    /// the file deleted to rotate it), when an explicit `DELTA_HOOK_SECRET`
    /// differs from the recorded one, or when the desktop app could not get its
    /// recorded port back and fell back to a fresh one. `false` otherwise,
    /// including the CLI's fixed port, which is never compared. Configuration
    /// built by hand (tests) leaves it `false`.
    ///
    /// It says whether surviving sessions *would* be stranded, not that any
    /// exist: a first run reads `true` with no session to strand, so a caller
    /// pairs it with the sessions it actually finds. Explicit overrides are
    /// blind spots, because neither `DELTA_PORT` nor `DELTA_HOOK_SECRET` is
    /// recorded: an explicit port is never compared, so moving to or from one
    /// reads `false`; and a secret is compared with the recorded one, not with
    /// the previous run's, so the same `DELTA_HOOK_SECRET` on every start reads
    /// `true` each time, while dropping it after a run that used it reads
    /// `false`.
    pub hook_endpoint_changed: bool,
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
    /// different ports — whose hook URLs differ — never share one file. With
    /// the port and hook secret kept across restarts, a restart lands on the
    /// same path and rewrites it with the same contents.
    pub fn session_settings_path(&self) -> String {
        std::env::temp_dir()
            .join(format!("delta-{}", self.port))
            .join("settings.json")
            .to_string_lossy()
            .into_owned()
    }
}
