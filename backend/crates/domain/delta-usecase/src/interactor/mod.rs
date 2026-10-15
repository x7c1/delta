//! The [`Interactor`]: orchestrates the ports into Delta's use cases.
//!
//! The use cases are split by area into child modules (`runtime`, `lifecycle`,
//! `enqueue`, `hooks`, `sync`, `listing`, `context`, `workdir`), each carrying
//! its own `impl` block. The injected capabilities live in
//! [`InteractorCore`]; the [`Interactor`] wraps the core in an [`Arc`] and
//! derefs to it, keeping the core's read paths callable directly on the
//! interactor. Per-session runtime state lives in the `session_actor` module
//! (one actor task per session), reached through the `routing` impl.

mod agent_event;
mod agent_permission;
mod answer_question;
mod cancel_question;
mod cancel_send;
mod context;
mod core_release;
mod echo_deadline;
mod enqueue;
mod hooks;
mod interactor_core;
mod interrupt;
mod launch_options;
mod lifecycle;
mod listing;
mod open_cwd;
mod permission_decision;
mod prompt_templates;
mod provider_availability;
mod pull_requests;
mod question_keys;
mod release_send;
mod repository;
mod routing;
mod runtime;
pub(crate) mod session_actor;
mod settle_pending_permissions;
mod storage;
mod sweep_running_subagents;
mod sync;
mod text_digest;
mod turn_input;
mod workdir;

pub use core_release::CoreRelease;
pub use hooks::PermissionWait;
pub use interactor_core::InteractorCore;
pub use open_cwd::{ExternalHandler, ExternalHandlerId, VSCODE_HANDLER_ID};
pub use permission_decision::PermissionDecision;
pub use routing::ReadoptionSummary;
pub use session_actor::runtime::{
    AttachablePane, PendingPermission, PendingQuestion, RunningSubagent, SessionLiveState,
};

#[cfg(test)]
mod testing;
/// The tmux fake, for the crate-root tests of [`crate::PaneTokenMinter`] and
/// [`crate::pane_token::PaneToken`], which probe tmux through it.
#[cfg(test)]
pub(crate) use testing::FakeTmux;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::agent::{AgentAdapterFactory, LaunchOptionVocabulary, NullLaunchOptionVocabulary};
use crate::launch_config::LaunchConfig;
use crate::pane_token_minter::PaneTokenMinter;
use crate::ports::{
    AsyncEventSink, BinaryDetector, ExternalOpener, GhCli, GitWorktree, SessionStore, TmuxDriver,
    Transcript, Workspace,
};
use crate::pull_request::PullRequest;
use crate::pull_request_lens::PullRequestLens;

use session_actor::registry::SessionRegistry;

/// How long a PR search result stays memoised before the next call
/// re-shells out. Short enough that a refresh on the user's timescale wins,
/// long enough that flipping tabs in the panel does not spam `gh`.
pub(crate) const PR_SEARCH_CACHE_TTL: Duration = Duration::from_secs(30);

/// The default Codex launch binary, used by the default/test interactor when no
/// Codex binary has been wired. Mirrors `codex-agent`'s `CodexLaunchConfig`
/// default (`codex`, resolved via `PATH`); the domain cannot depend on that
/// gateway crate, so the fallback name is kept in sync here. Production always
/// overrides it via [`Interactor::with_codex_bin`] with the same value handed to
/// the Codex adapter factory.
pub(crate) const DEFAULT_CODEX_COMMAND: &str = "codex";

/// Per-process memo of one lens's PR search result.
///
/// Held under [`InteractorCore::pr_search_cache`] so toggling between
/// lenses (or re-mounting the PR tab) does not re-shell out within the
/// TTL. Each lens caches independently — the two lenses' result sets are
/// largely disjoint and a stale reviewer list should not block a fresh
/// author refresh.
pub(crate) struct PrSearchCacheEntry {
    pub(crate) fetched_at: Instant,
    pub(crate) pull_requests: Vec<PullRequest>,
}

/// The public entry point: wraps the shared `InteractorCore` and routes
/// per-session work to the session actors.
///
/// Derefs to the core, so the pure read paths (listing, threads, messages,
/// workdir browsing) implemented on the core are callable directly on the
/// interactor with no actor round-trip. Everything that touches a session's
/// runtime state — the pane binding, launch state, turn machine, permission
/// waiters, and transcript ingestion — goes through that session's actor
/// mailbox instead (see the `session_actor` module and the `routing` impl).
pub struct Interactor<T, X, S, W, G> {
    core: Arc<InteractorCore<T, X, S, W, G>>,
    /// session_id → actor mailbox; actors spawn on first contact.
    pub(in crate::interactor) sessions: SessionRegistry<T, X, S, W, G>,
}

impl<T, X, S, W, G> std::ops::Deref for Interactor<T, X, S, W, G> {
    type Target = InteractorCore<T, X, S, W, G>;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

/// An [`Interactor`] with its five ports type-erased behind trait objects.
///
/// Both the production composition root and integration tests build this exact
/// type, so the transport layer's shared state stays non-generic regardless of
/// which gateways are wired in.
pub type BoxedInteractor = Interactor<
    Box<dyn TmuxDriver>,
    Box<dyn Transcript>,
    Box<dyn SessionStore>,
    Box<dyn Workspace>,
    Box<dyn GitWorktree>,
>;

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Construct an Interactor from the five injected ports plus the spawn
    /// configuration (the base working directory, the worktree base directory,
    /// the rendered settings JSON, and the Delta-owned path that JSON is written
    /// to for `--settings`).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tmux: T,
        transcript: X,
        store: S,
        workspace: W,
        git_worktree: G,
        session_workdir_base: impl Into<String>,
        worktree_base: impl Into<String>,
        session_settings_json: impl Into<String>,
        session_settings_path: impl Into<String>,
    ) -> Self {
        let core = Arc::new(InteractorCore {
            tmux,
            transcript,
            store,
            workspace,
            git_worktree,
            session_workdir_base: session_workdir_base.into(),
            worktree_base: worktree_base.into(),
            transcript_root: None,
            session_settings_json: session_settings_json.into(),
            session_settings_path: session_settings_path.into(),
            launch: LaunchConfig::default(),
            minter: PaneTokenMinter::new(),
            repository_origin_cache: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            gh_cli: Arc::new(UnavailableGhCli),
            external_opener: Arc::new(UnwiredExternalOpener),
            adapter_factories: HashMap::new(),
            launch_option_vocabulary: Arc::new(NullLaunchOptionVocabulary),
            binary_detector: Arc::new(UnwiredBinaryDetector),
            codex_bin: DEFAULT_CODEX_COMMAND.to_owned(),
            event_sink: None,
            pr_search_cache: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            permission_index: std::sync::Mutex::new(HashMap::new()),
            clone_jobs: repository::CloneJobs::default(),
            #[cfg(test)]
            launches_in_flight: std::sync::atomic::AtomicUsize::new(0),
        });
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }

    /// A handle that tells when this interactor's shared core — every port,
    /// the store's connection included — has been dropped, for a caller that
    /// drops the interactor and must then wait for the session actors to run
    /// down before touching the files the ports held open.
    pub fn core_release(&self) -> CoreRelease {
        let core: Arc<dyn std::any::Any + Send + Sync> = self.core.clone();
        CoreRelease::new(Arc::downgrade(&core))
    }

    /// Replace the launch configuration (binary to spawn, watchdog deadlines).
    ///
    /// A builder-style override so the many existing constructor call sites
    /// keep the production defaults without naming them; the composition root
    /// applies whatever the environment configured. Must run before any
    /// session actor is spawned, i.e. right after [`Self::new`] — the core is
    /// rebuilt here, which would strand an already-running actor's registry.
    pub fn with_launch_config(self, launch: LaunchConfig) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!("with_launch_config must be called before any session actor is spawned");
        };
        core.launch = launch;
        let core = Arc::new(core);
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }

    /// Confine hook-reported transcript paths to `root`.
    ///
    /// Turns on the confinement `register_session_row` enforces: an incoming
    /// `transcript_path` must be a `.jsonl` file resolving under `root` or the
    /// registration is refused with [`crate::Error::InvalidTranscriptPath`],
    /// before the path is persisted or read. `root` itself has to exist; the
    /// reported path need not, since its per-project directory is created only
    /// after the hook fires. The composition root passes
    /// `Config::transcript_root`; a configuration that never calls this (the
    /// default constructor, the domain tests) leaves confinement off. Same
    /// constraint as [`Self::with_launch_config`]: must run before any session
    /// actor is spawned.
    pub fn with_transcript_root(self, root: impl Into<String>) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!("with_transcript_root must be called before any session actor is spawned");
        };
        core.transcript_root = Some(root.into());
        let core = Arc::new(core);
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }

    /// Inject the `gh` CLI driver for the PR tab.
    ///
    /// A builder-style override sibling of [`Self::with_launch_config`]:
    /// the default constructor wires a stub driver that reports gh as
    /// unavailable, so a configuration that does not care about the PR
    /// tab (the existing call sites, tests) keeps working with no
    /// changes; production wiring replaces it. Same constraint as
    /// [`Self::with_launch_config`]: must run before any session actor is
    /// spawned.
    pub fn with_gh_cli(self, gh_cli: Arc<dyn GhCli>) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!("with_gh_cli must be called before any session actor is spawned");
        };
        core.gh_cli = gh_cli;
        let core = Arc::new(core);
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }

    /// Inject the [`ExternalOpener`] driver for the `open cwd` endpoint.
    ///
    /// The default constructor wires an unwired stub that fails every open
    /// call, so a configuration that has not wired the real opener (existing
    /// tests, dev harnesses) is safe by default — the failure is loud and
    /// clearly attributed to missing wiring rather than silently succeeding
    /// or crashing on an unrelated path. Same constraint as
    /// [`Self::with_launch_config`]: must run before any session actor is
    /// spawned.
    pub fn with_external_opener(self, opener: Arc<dyn ExternalOpener>) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!("with_external_opener must be called before any session actor is spawned");
        };
        core.external_opener = opener;
        let core = Arc::new(core);
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }

    /// Register the factory that lazily builds a provider's
    /// [`AgentAdapter`](crate::AgentAdapter),
    /// keyed by the provider it drives ([`AgentAdapterFactory::provider`]).
    ///
    /// The default constructor registers no factory, so a configuration that
    /// does not drive an adapter-backed provider — the existing call sites,
    /// tests — is unaffected. Production wiring registers one factory per
    /// adapter-backed provider (currently Codex), each carrying only its
    /// launch configuration; no provider process is spawned here, so a machine
    /// without the provider installed still starts normally. Registering a
    /// second factory for the same provider replaces the first. Same
    /// constraint as [`Self::with_launch_config`]: must run before any session
    /// actor is spawned.
    pub fn with_adapter_factory(self, factory: Arc<dyn AgentAdapterFactory>) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!("with_adapter_factory must be called before any session actor is spawned");
        };
        core.adapter_factories.insert(factory.provider(), factory);
        let core = Arc::new(core);
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }

    /// Inject the [`LaunchOptionVocabulary`] that classifies launch options in
    /// each agent's vocabulary: which ones disable an agent's own safety
    /// mechanisms, and which names take a single value.
    ///
    /// The default constructor wires [`NullLaunchOptionVocabulary`], so a
    /// configuration that has not injected the real vocabulary (the domain
    /// tests, dev harnesses) classifies nothing as dangerous and every name as
    /// repeatable, and behaves as it did before those rules existed. Production
    /// wiring installs the gateway-backed vocabulary.
    /// Same constraint as [`Self::with_launch_config`]: must run before any
    /// session actor is spawned.
    pub fn with_launch_option_vocabulary(
        self,
        vocabulary: Arc<dyn LaunchOptionVocabulary>,
    ) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!(
                "with_launch_option_vocabulary must be called before any session actor is spawned"
            );
        };
        core.launch_option_vocabulary = vocabulary;
        let core = Arc::new(core);
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }

    /// Inject the [`BinaryDetector`] used by the `/api/providers` availability
    /// endpoint.
    ///
    /// The default constructor wires a stub that reports every binary as absent,
    /// so a configuration that has not wired the real probe (existing tests, dev
    /// harnesses) is safe by default. Production wiring replaces it with the
    /// PATH probe. Same constraint as [`Self::with_launch_config`]: must run
    /// before any session actor is spawned.
    pub fn with_binary_detector(self, detector: Arc<dyn BinaryDetector>) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!("with_binary_detector must be called before any session actor is spawned");
        };
        core.binary_detector = detector;
        let core = Arc::new(core);
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }

    /// Set the Codex launch binary the availability endpoint probes.
    ///
    /// A builder-style override sibling of [`Self::with_launch_config`] (which
    /// carries the Claude binary): the composition root passes the same value it
    /// hands the Codex adapter factory, so availability probes exactly the
    /// binary a Codex spawn would use. Defaults to `codex` when not set. Same
    /// constraint as [`Self::with_launch_config`]: must run before any session
    /// actor is spawned.
    pub fn with_codex_bin(self, codex_bin: impl Into<String>) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!("with_codex_bin must be called before any session actor is spawned");
        };
        core.codex_bin = codex_bin.into();
        let core = Arc::new(core);
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }

    /// Inject the async event-emission [`AsyncEventSink`].
    ///
    /// The default constructor holds no sink (`None`), so a configuration that
    /// never drives an async producer — the existing call sites, the domain
    /// tests — is unaffected and any async emit is silently dropped. The server
    /// wires the sink whose receiver its drain task forwards to the broadcast.
    /// Same constraint as [`Self::with_launch_config`]: must run before any
    /// session actor is spawned.
    pub fn with_event_sink(self, sink: AsyncEventSink) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!("with_event_sink must be called before any session actor is spawned");
        };
        core.event_sink = Some(sink);
        let core = Arc::new(core);
        let sessions = SessionRegistry::new(&core);
        Self { core, sessions }
    }
}

/// Stub `gh` driver wired by [`Interactor::new`] when no real driver has
/// been injected yet.
///
/// Reports gh as unavailable and an empty result list, so the PR tab
/// renders its "run `gh auth login`" hint instead of crashing the server.
/// Production wiring replaces this through [`Interactor::with_gh_cli`].
struct UnavailableGhCli;

#[async_trait::async_trait]
impl GhCli for UnavailableGhCli {
    async fn is_authenticated(&self) -> bool {
        false
    }

    async fn search_prs(&self, _lens: PullRequestLens) -> crate::error::Result<Vec<PullRequest>> {
        Ok(Vec::new())
    }

    async fn clone_repo(
        &self,
        owner: &str,
        name: &str,
        _destination: &str,
    ) -> crate::error::Result<()> {
        // Unlike the queries, a clone cannot degrade into "nothing to show":
        // the caller asked for a directory to appear on disk. Fail loudly and
        // name the missing wiring, so a mis-configured server says so instead
        // of announcing a clone that never ran as finished.
        Err(crate::error::Error::Gh(format!(
            "no gh driver has been injected into the interactor, so {owner}/{name} cannot be cloned"
        )))
    }
}

/// Stub [`ExternalOpener`] wired by [`Interactor::new`] when no real driver
/// has been injected yet.
///
/// Every open call reports [`crate::Error::ExternalOpenerSpawnFailed`] with
/// a message that names the missing wiring, so a `POST /api/open-cwd` request
/// against a mis-configured server surfaces the mistake immediately instead
/// of appearing to succeed. Production wiring replaces this through
/// [`Interactor::with_external_opener`].
struct UnwiredExternalOpener;

#[async_trait::async_trait]
impl ExternalOpener for UnwiredExternalOpener {
    async fn open(&self, _command: &str, _args: Vec<String>) -> crate::error::Result<()> {
        Err(crate::error::Error::ExternalOpenerSpawnFailed(
            "no ExternalOpener driver has been injected into the interactor".to_owned(),
        ))
    }
}

/// Stub [`BinaryDetector`] wired by [`Interactor::new`] when no real probe has
/// been injected yet.
///
/// Reports every binary as absent, so a configuration that has not wired the
/// real PATH probe (existing tests, dev harnesses) reports providers as
/// unavailable rather than falsely claiming a binary is present. Production
/// wiring replaces this through [`Interactor::with_binary_detector`].
struct UnwiredBinaryDetector;

#[async_trait::async_trait]
impl BinaryDetector for UnwiredBinaryDetector {
    async fn is_available(&self, _bin: &str) -> bool {
        false
    }
}
