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

// Re-export the underlying store error so callers (the `delta-server` binary)
// can pattern-match on its variants — notably `SchemaMismatch`, which it
// surfaces with a clean message at startup — without taking a direct
// dependency on `delta-sqlite`.
pub use delta_sqlite::Error as StoreError;

// Re-exported so a binary that only configures the server (a test harness,
// the `delta-server` main) can name the launch settings without depending on
// the use-case crate directly.
pub use delta_usecase::LaunchConfig;

use std::sync::Arc;

use binary_detector::PathBinaryDetector;
use claude_agent::CLAUDE_CAPABILITIES;
use codex_agent::{CodexAdapterFactory, CodexLaunchConfig, CODEX_CAPABILITIES};
use delta_sqlite::SqliteStore;
use delta_transcript::JsonlTranscript;
use delta_usecase::{
    AgentAdapterFactory, AgentCapabilities, AgentProvider, BinaryDetector, BoxedInteractor,
    ExternalOpener, GhCli, Interactor,
};
use external_opener::SystemOpener;
use gh_cli::Gh;
use git_worktree::Git;
use tmux_driver::Tmux;
use workspace_fs::FsWorkspace;

/// The fully-wired Interactor.
///
/// The gateways are type-erased behind trait objects so the transport layer's
/// shared state is a single non-generic type, shared between this production
/// wiring and the integration tests that substitute fakes.
pub type AppInteractor = BoxedInteractor;

/// The static capability profile for a provider, resolved *without* a live
/// adapter instance.
///
/// Each provider's profile is declared once in its gateway adapter (the
/// `*_CAPABILITIES` const its [`AgentAdapter::capabilities`] returns) and read
/// back here through the same const, so the value the REST layer surfaces can
/// never drift from what a running adapter reports. The composition root is the
/// natural home: it is the one layer that already knows every gateway adapter,
/// and callers (e.g. `GET /api/providers`) need a provider's profile before —
/// or entirely without — an adapter being spawned.
///
/// Adding a provider is a new [`AgentProvider`] variant plus its capability
/// profile in the gateway layer plus a new arm here — the same fan-out the
/// availability probe documents.
///
/// [`AgentAdapter::capabilities`]: delta_usecase::AgentAdapter::capabilities
pub fn provider_capabilities(provider: AgentProvider) -> AgentCapabilities {
    match provider {
        AgentProvider::Claude => CLAUDE_CAPABILITIES,
        AgentProvider::Codex => CODEX_CAPABILITIES,
    }
}

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
        render_session_settings(self.port)
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

/// Construct the wired [`AppInteractor`] from configuration.
///
/// Opening the store applies the schema migration. The transcript path is not
/// needed here — it is learned from the first `UserPromptSubmit` hook. The
/// stateless [`Tmux`] driver mints a fresh tmux session per spawn, so no fixed
/// session name is configured.
///
/// Boot-time send reconcile: every `dispatched` row surviving from the
/// previous process is restored here — returned to `queued` with the
/// `restored_at` marker set — before any session actor exists. A restored
/// row stays visible in the open-send list but never auto-dispatches; the
/// user explicitly releases (or cancels) it from the UI. See
/// [`SessionStore::restore_all_dispatched`] for why the sweep is exact at
/// that moment and why the rows are restored rather than requeued or
/// cancelled.
///
/// [`SessionStore::restore_all_dispatched`]: delta_usecase::SessionStore::restore_all_dispatched
pub async fn build(config: &Config) -> Result<AppInteractor> {
    let store = SqliteStore::open(&config.database_path)?;
    let restored = delta_usecase::SessionStore::restore_all_dispatched(&store).await?;
    if restored > 0 {
        tracing::info!(
            restored,
            "restored dispatched sends orphaned by the previous process; \
             they await an explicit release or cancel from the UI"
        );
    }
    let transcript = JsonlTranscript::new();
    let tmux = Tmux::new(config.tmux_socket.clone());
    let workspace = FsWorkspace::new();
    let git_worktree = Git::new();
    let gh_cli: Arc<dyn GhCli> = Arc::new(Gh::new());
    let external_opener: Arc<dyn ExternalOpener> = Arc::new(SystemOpener::new());
    // Held but dormant: the factory carries only Codex launch config, so this
    // spawns no `codex app-server` process at startup — a machine without Codex
    // still boots normally. Nothing consults it yet; provider dispatch that
    // calls `connect()` lands in a later change.
    // Resolve the Codex launch config once and reuse its binary for both the
    // adapter factory (what a Codex spawn launches) and the availability probe
    // (what `/api/providers` reports), so the two can never diverge.
    let codex_launch = codex_launch_from_env();
    let codex_bin = codex_launch.codex_bin.clone();
    let codex_adapter_factory: Arc<dyn AgentAdapterFactory> =
        Arc::new(CodexAdapterFactory::new(codex_launch));
    // Real PATH probe for the provider-availability endpoint. Constructing it
    // touches no filesystem; the first probe per binary does, then memoises.
    let binary_detector: Arc<dyn BinaryDetector> = Arc::new(PathBinaryDetector::new());
    Ok(Interactor::new(
        Box::new(tmux) as Box<dyn delta_usecase::TmuxDriver>,
        Box::new(transcript) as Box<dyn delta_usecase::Transcript>,
        Box::new(store) as Box<dyn delta_usecase::SessionStore>,
        Box::new(workspace) as Box<dyn delta_usecase::Workspace>,
        Box::new(git_worktree) as Box<dyn delta_usecase::GitWorktree>,
        config.session_workdir_base.clone(),
        config.worktree_base.clone(),
        config.session_settings_json(),
        config.session_settings_path(),
    )
    .with_launch_config(config.launch.clone())
    .with_gh_cli(gh_cli)
    .with_external_opener(external_opener)
    .with_codex_adapter_factory(codex_adapter_factory)
    .with_codex_bin(codex_bin)
    .with_binary_detector(binary_detector))
}

/// The Codex launch configuration sourced from the environment.
///
/// `DELTA_CODEX_BIN` substitutes the `codex` command the shared app-server is
/// spawned from (default the bare `codex`, resolved via `PATH`), mirroring
/// `DELTA_CLAUDE_BIN` for the Claude launch. Only the binary is configurable in
/// this slice; the default `app-server` argument is kept.
///
/// Read here in the composition root — rather than threaded through [`Config`]
/// — so every existing `Config` construction stays untouched. Reading the
/// variable has no side effect: the resulting config is only stored on the
/// factory and no process is spawned until a Codex session needs one.
fn codex_launch_from_env() -> CodexLaunchConfig {
    let mut codex = CodexLaunchConfig::default();
    if let Ok(bin) = std::env::var("DELTA_CODEX_BIN") {
        if !bin.is_empty() {
            codex.codex_bin = bin;
        }
    }
    codex
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            database_path: ":memory:".into(),
            session_workdir_base: "/tmp/delta-session".into(),
            worktree_base: "/tmp/delta-worktrees".into(),
            tmux_socket: DEFAULT_TMUX_SOCKET.into(),
            port: 7878,
            launch: delta_usecase::LaunchConfig::default(),
        }
    }

    #[tokio::test]
    async fn build_wires_an_interactor_with_in_memory_store() {
        assert!(build(&test_config()).await.is_ok());
    }

    /// The static accessor resolves each provider's terminal capability without
    /// a live adapter: Claude offers an attachable PTY, Codex has no terminal.
    /// This is the fact the workspace's terminal gating hangs on — a Codex
    /// session must never show a terminal tab.
    #[test]
    fn provider_capabilities_report_the_terminal_surface_per_provider() {
        use delta_usecase::TerminalCapability;

        assert_eq!(
            provider_capabilities(AgentProvider::Claude).terminal,
            TerminalCapability::AttachablePty,
        );
        assert_eq!(
            provider_capabilities(AgentProvider::Codex).terminal,
            TerminalCapability::NoTerminal,
        );
    }

    /// The accessor returns exactly what each adapter's `capabilities()` returns
    /// — the guarantee that the REST-surfaced profile can never drift from a
    /// running adapter's. Asserted against the adapter consts directly (both are
    /// the single source of truth the accessor reads).
    #[test]
    fn provider_capabilities_match_the_adapter_source_of_truth() {
        assert_eq!(
            provider_capabilities(AgentProvider::Claude),
            CLAUDE_CAPABILITIES,
        );
        assert_eq!(
            provider_capabilities(AgentProvider::Codex),
            CODEX_CAPABILITIES,
        );
    }

    /// The boot-time send reconcile is wired into [`build`] itself, not just
    /// available on the store: a row a previous process left `dispatched` is
    /// `queued` **and marked restored** once `build` has run against the same
    /// database file. (The store-level sweep semantics are pinned in
    /// `delta-sqlite`; this pins the composition root actually invoking it at
    /// startup — the sweep being skipped would reintroduce the restart zombie
    /// while every store-level test stayed green.)
    #[tokio::test]
    async fn build_restores_dispatched_sends_left_by_a_previous_process() {
        use delta_model::SendStatus;
        use delta_usecase::{NewSession, SessionStore};

        let dir = std::env::temp_dir().join(format!("delta-bootstrap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("boot-reconcile.sqlite");
        let _ = std::fs::remove_file(&path);
        let path_str = path.to_str().unwrap().to_owned();

        // The "previous process": register a session, leave one send
        // `dispatched` (what `enqueue_send` writes), and drop the connection.
        let stale_id = {
            let store = SqliteStore::open(&path_str).unwrap();
            let (session, main) = store
                .register_session(NewSession {
                    id: "sess-1".into(),
                    cwd: "/work".into(),
                    transcript_path: "/tmp/t.jsonl".into(),
                    branch_at_launch: None,
                    repo_root: None,
                    repository_display_name: None,
                })
                .await
                .unwrap();
            let stale = store
                .enqueue_send(&session.id, main, None, "stale prompt", None)
                .await
                .unwrap();
            assert_eq!(stale.status, SendStatus::Dispatched);
            stale.id
        };

        // The next process boots against the same file. The returned
        // interactor is dropped at the end of the statement, releasing its
        // connection before the verification re-open below.
        let config = Config {
            database_path: path_str.clone(),
            ..test_config()
        };
        build(&config).await.unwrap();

        let store = SqliteStore::open(&path_str).unwrap();
        let stale = store.send(stale_id).await.unwrap().unwrap();
        assert_eq!(
            stale.status,
            SendStatus::Queued,
            "boot returns the orphaned dispatched row to queued"
        );
        assert!(
            stale.restored_at.is_some(),
            "the restored marker is set, so the row awaits an explicit release"
        );
    }
}
