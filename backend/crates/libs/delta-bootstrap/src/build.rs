//! The composition root's wiring: [`build`] and the environment it reads.

use std::sync::Arc;

use binary_detector::PathBinaryDetector;
use codex_agent::{CodexAdapterFactory, CodexLaunchConfig};
use delta_sqlite::SqliteStore;
use delta_transcript::JsonlTranscript;
use delta_usecase::{
    AgentAdapterFactory, BinaryDetector, CommsLogSink, ExternalOpener, GhCli, Interactor,
    LaunchOptionVocabulary,
};
use external_opener::SystemOpener;
use gh_cli::Gh;
use git_worktree::Git;
use tmux_driver::Tmux;
use workspace_fs::FsWorkspace;

use crate::ensure_tmux_available::ensure_tmux_available;
use crate::{
    all_launch_option_presets, AppInteractor, Config, Error, GatewayLaunchOptionVocabulary, Result,
};

/// Construct the wired [`AppInteractor`] from configuration.
///
/// Opening the store applies the schema migration. The transcript path is not
/// needed here — it is learned from the first `UserPromptSubmit` hook. The
/// stateless [`Tmux`] driver mints a fresh tmux session per spawn, so no fixed
/// session name is configured.
///
/// `comms_log` receives the JSON-RPC frames every adapter-driven session
/// exchanges with its provider, for the browser's comms-log inspector. Passed in
/// (rather than created here) because the transport layer serves the same
/// instance on `/comms`; a caller with no inspector passes
/// [`delta_usecase::NullCommsLog`].
///
/// Boot-time send reconcile: every `dispatched` row surviving from the
/// previous process is restored here — returned to `queued` with the
/// `held_at` marker set — before any session actor exists. A restored
/// row stays visible in the open-send list but never auto-dispatches; the
/// user explicitly releases (or cancels) it from the UI. See
/// [`SessionStore::restore_all_dispatched`] for why the sweep is exact at
/// that moment and why the rows are restored rather than requeued or
/// cancelled.
///
/// Boot-time launch-option reconcile: the launch options Delta *ships* — the
/// declared per-provider catalogs, read through [`launch_option_catalog`] — are
/// materialized into the registry here too, so a shipped option is already a
/// real row the first time Settings is opened, and reappears by itself after a
/// database reset. Idempotent, and it preserves each row's `default_enabled`
/// flag, which is the user's — see the use case's
/// `reconcile_builtin_launch_options` for why that one field is the only thing
/// it has to protect.
///
/// Host requirements: `tmux` must be resolvable before anything else is wired.
/// See [`ensure_tmux_available()`] for why that is checked here rather than
/// left to the first launch. The probe reads the process's real `PATH` and
/// takes no substitute detector, so every caller needs tmux installed — the
/// tests that wire this root (the server's route tests among them) as much as
/// a real boot.
///
/// [`SessionStore::restore_all_dispatched`]: delta_usecase::SessionStore::restore_all_dispatched
/// [`launch_option_catalog`]: crate::launch_option_catalog()
pub async fn build(config: &Config, comms_log: Arc<dyn CommsLogSink>) -> Result<AppInteractor> {
    // Real PATH probe. Constructing it touches no filesystem; the first probe
    // per binary does, then memoises. Built here, ahead of the store, because
    // the startup tmux check below is the first thing it serves; the same
    // instance is injected into the interactor for the provider-availability
    // endpoint, so both read one memo.
    let binary_detector: Arc<dyn BinaryDetector> = Arc::new(PathBinaryDetector::new());
    ensure_tmux_available(binary_detector.as_ref()).await?;
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
    // The factory carries only Codex launch config, so this spawns no `codex
    // app-server` process at startup — a machine without Codex still boots
    // normally; the spawn is deferred to the first Codex session's `connect()`.
    // Registered into the interactor's adapter-factory registry below, which is
    // what dispatches a Codex session onto the terminal-less adapter path.
    // Resolve the Codex launch config once and reuse its binary for both the
    // adapter factory (what a Codex spawn launches) and the availability probe
    // (what `/api/providers` reports), so the two can never diverge.
    let codex_launch = codex_launch_from_env();
    let codex_bin = codex_launch.codex_bin.clone();
    // The comms log is injected here rather than resolved inside the adapter: it
    // is an observability gateway the transport layer owns (it is also what
    // serves `/comms`), and a provider whose wire is not inspectable simply never
    // records into it — so no per-provider branch appears anywhere.
    let codex_adapter_factory: Arc<dyn AgentAdapterFactory> =
        Arc::new(CodexAdapterFactory::new(codex_launch).with_comms_log(comms_log));
    let interactor = Interactor::new(
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
    .with_transcript_root(config.transcript_root.clone())
    .with_gh_cli(gh_cli)
    .with_external_opener(external_opener)
    .with_adapter_factory(codex_adapter_factory)
    .with_codex_bin(codex_bin)
    .with_binary_detector(binary_detector)
    .with_launch_option_vocabulary(
        Arc::new(GatewayLaunchOptionVocabulary) as Arc<dyn LaunchOptionVocabulary>
    );
    // Boot-time launch-option reconcile (see this function's doc), in the same
    // place as the send sweep above. The whole declared catalog in one call,
    // because reconciliation's closing sweep is registry-wide — see
    // `all_launch_option_presets`.
    interactor
        .reconcile_builtin_launch_options(&all_launch_option_presets())
        .await
        .map_err(Error::BuiltinLaunchOptions)?;
    Ok(interactor)
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

    use delta_model::AgentProvider;
    use delta_usecase::NullCommsLog;

    use crate::DEFAULT_TMUX_SOCKET;

    fn test_config() -> Config {
        Config {
            database_path: ":memory:".into(),
            session_workdir_base: "/tmp/delta-session".into(),
            worktree_base: "/tmp/delta-worktrees".into(),
            tmux_socket: DEFAULT_TMUX_SOCKET.into(),
            auth_token: "test-token".into(),
            hook_secret: "test-hook-secret".into(),
            transcript_root: "/tmp".into(),
            port: 7878,
            launch: delta_usecase::LaunchConfig::default(),
        }
    }

    /// Wiring succeeds against an in-memory store.
    ///
    /// Runs the real host-requirement probe (see [`build`]), so it needs tmux on
    /// the test host's `PATH`. CI installs it for the backend job.
    #[tokio::test]
    async fn build_wires_an_interactor_with_in_memory_store() {
        assert!(
            build(&test_config(), NullCommsLog::arc()).await.is_ok(),
            "wiring failed; if this host has no tmux on PATH that is the cause — \
             `build` refuses to wire without it"
        );
    }

    /// The wired vocabulary is the gateway classifications, per provider — the
    /// danger predicate reads the pair, not the name alone.
    ///
    /// Pinned through the port (not the free function) because the port is what
    /// the domain consults; a `build` that forgot to inject it would leave the
    /// permissive default in place with every gateway test still green.
    #[tokio::test]
    async fn build_wires_the_gateway_launch_option_vocabulary() {
        let interactor = build(&test_config(), NullCommsLog::arc()).await.unwrap();
        let dangerous = |provider, name: &str, value: Option<&str>| {
            interactor.is_launch_option_pair_dangerous(provider, name, value)
        };

        assert!(dangerous(
            AgentProvider::Claude,
            "--dangerously-skip-permissions",
            None
        ));
        assert!(dangerous(
            AgentProvider::Codex,
            "sandbox",
            Some("danger-full-access")
        ));
        // The benign neighbours of both, so the policy is not simply "yes".
        assert!(!dangerous(AgentProvider::Claude, "--model", Some("opus")));
        assert!(!dangerous(
            AgentProvider::Codex,
            "sandbox",
            Some("read-only")
        ));
        // And a Claude flag is read in Claude's vocabulary alone: the same text
        // means nothing to Codex.
        assert!(!dangerous(
            AgentProvider::Codex,
            "--dangerously-skip-permissions",
            None
        ));

        // Cardinality goes through the same port: each provider's default and
        // its exception.
        let cardinality =
            |provider, name: &str| interactor.launch_option_cardinality(provider, name);
        assert_eq!(
            cardinality(AgentProvider::Claude, "--model"),
            delta_usecase::LaunchOptionCardinality::Single
        );
        assert_eq!(
            cardinality(AgentProvider::Claude, "--plugin-dir"),
            delta_usecase::LaunchOptionCardinality::Multiple
        );
        assert_eq!(
            cardinality(AgentProvider::Codex, "config"),
            delta_usecase::LaunchOptionCardinality::Multiple
        );
    }

    /// [`build`] materializes every declared preset, so a shipped launch option
    /// is in the registry before the browser has asked for anything — and a
    /// second boot against the same file leaves the same rows, ids included.
    ///
    /// This is what makes the shipped rows come back by themselves after a
    /// `make reset`, and the ids stable across an ordinary restart.
    #[tokio::test]
    async fn build_materializes_the_declared_launch_option_catalog() {
        use delta_usecase::SessionStore;

        let dir = std::env::temp_dir().join(format!("delta-bootstrap-lo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("builtin-launch-options.sqlite");
        let _ = std::fs::remove_file(&path);
        let config = Config {
            database_path: path.to_str().unwrap().to_owned(),
            ..test_config()
        };

        build(&config, NullCommsLog::arc()).await.unwrap();
        let first = SqliteStore::open(&config.database_path)
            .unwrap()
            .list_launch_options()
            .await
            .unwrap();

        let declared = all_launch_option_presets();
        assert_eq!(first.len(), declared.len());
        for preset in &declared {
            let row = first
                .iter()
                .find(|row| row.builtin_key.as_deref() == Some(preset.key))
                .unwrap_or_else(|| panic!("`{}` was not materialized", preset.key));
            assert_eq!(row.label.as_deref(), Some(preset.label));
            assert_eq!(row.name, preset.name);
            assert_eq!(row.value.as_deref(), preset.value);
            assert_eq!(row.provider, preset.provider);
            assert!(!row.default_enabled, "offered, not imposed");
        }

        // A second boot is a no-op, ids included.
        build(&config, NullCommsLog::arc()).await.unwrap();
        let second = SqliteStore::open(&config.database_path)
            .unwrap()
            .list_launch_options()
            .await
            .unwrap();
        assert_eq!(first, second);
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
        build(&config, NullCommsLog::arc()).await.unwrap();

        let store = SqliteStore::open(&path_str).unwrap();
        let stale = store.send(stale_id).await.unwrap().unwrap();
        assert_eq!(
            stale.status,
            SendStatus::Queued,
            "boot returns the orphaned dispatched row to queued"
        );
        assert!(
            stale.held_at.is_some(),
            "the restored marker is set, so the row awaits an explicit release"
        );
    }
}
