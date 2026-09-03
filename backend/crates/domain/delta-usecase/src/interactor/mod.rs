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
mod echo_deadline;
mod enqueue;
mod hooks;
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
mod sweep_running_subagents;
mod sync;
mod turn_input;
mod workdir;

pub use hooks::PermissionWait;
pub use open_cwd::{ExternalHandler, ExternalHandlerId, VSCODE_HANDLER_ID};
pub use permission_decision::PermissionDecision;
pub use session_actor::runtime::{
    PendingPermission, PendingQuestion, RunningSubagent, SessionLiveState,
};

#[cfg(test)]
mod testing;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use delta_model::{AgentProvider, SessionId};

use crate::agent::{AgentAdapterFactory, LaunchOptionDangerPolicy, NoDangerousLaunchOptions};
use crate::launch_config::LaunchConfig;
use crate::pane_token::PaneTokenMinter;
use crate::ports::{
    AsyncEventSink, BinaryDetector, ExternalOpener, GhCli, GitWorktree, SessionEvent, SessionStore,
    TmuxDriver, Transcript, Workspace,
};
use crate::pull_request::{PullRequest, PullRequestLens};

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

/// Holds the injected capabilities and implements Delta's use cases.
///
/// Generic over the five ports so callers can inject any implementation. This
/// is the shared core behind [`Interactor`]: the interactor (and any
/// background task it spawns) holds it through an [`Arc`], so the core itself
/// carries no registry of those tasks — only the capabilities and the
/// process-runtime state.
pub struct InteractorCore<T, X, S, W, G> {
    pub(in crate::interactor) tmux: T,
    pub(in crate::interactor) transcript: X,
    pub(in crate::interactor) store: S,
    pub(in crate::interactor) workspace: W,
    pub(in crate::interactor) git_worktree: G,
    /// Base directory for per-spawn working directories.
    ///
    /// Each fresh spawn runs in its own `<base>/<token>` subdirectory. The
    /// workdir is no longer the hook-binding key — correlation is by the
    /// Delta-minted session id pinned via `claude --session-id` — so this base
    /// is free to become a user-selected project path in a later change without
    /// breaking spawn↔session correlation.
    pub(in crate::interactor) session_workdir_base: String,
    /// Base directory for per-session git worktrees
    /// (`<base>/<org>-<repo>-<session-id>`, where `<org>-<repo>` is the
    /// repository-identity slug).
    ///
    /// Kept separate from [`Self::session_workdir_base`] and pointed at a
    /// neutral location *outside* any repository tree: Claude Code discovers
    /// `CLAUDE.md`/settings by walking up from its cwd, so a worktree nested
    /// under another repo would inherit that repo's config. Used only for
    /// worktree sessions; default per-token spawns still use
    /// [`Self::session_workdir_base`].
    pub(in crate::interactor) worktree_base: String,
    /// The directory a hook-reported `transcript_path` must resolve under to be
    /// registered (and therefore later read and surfaced to the browser).
    ///
    /// `Some(root)` turns on confinement: `register_session_row` refuses
    /// anything that is not a `.jsonl` file resolving under this root, so a
    /// forged (or first, unvalidated) hook cannot point Delta's transcript
    /// tailer at an arbitrary file. The path itself need not exist yet — Claude
    /// Code creates the per-project directory only on its first transcript
    /// write, after the `SessionStart` hook — so it is this root and the
    /// deepest *existing* ancestor of the reported path that get canonicalized
    /// (see `hooks::validate_transcript_path`). Production always wires it (from
    /// `Config::transcript_root`); `None` — the default constructor used by the
    /// domain tests, which register sessions with throwaway temp paths — skips
    /// the check, exactly as the other unwired capabilities default to a
    /// permissive stub. See [`Interactor::with_transcript_root`].
    pub(in crate::interactor) transcript_root: Option<String>,
    /// The Claude Code settings JSON whose hooks point back at this server (and
    /// which pins the session theme). Rendered by the caller (with the running
    /// port) and held verbatim; written to [`Self::session_settings_path`] and
    /// passed to `claude --settings`.
    pub(in crate::interactor) session_settings_json: String,
    /// Delta-owned path the settings JSON is written to before each launch, then
    /// passed to `claude --settings <path>`. Kept *outside* any session working
    /// directory so spawning/resuming in a real project never overwrites that
    /// project's own `.claude/settings.json`.
    pub(in crate::interactor) session_settings_path: String,
    /// How sessions are launched (which binary) and how long the watchdog
    /// waits on a launch. Production defaults via [`LaunchConfig::default`];
    /// overridden through [`Interactor::with_launch_config`].
    pub(in crate::interactor) launch: LaunchConfig,
    /// Mints unique [`PaneToken`]s for fresh spawns.
    ///
    /// [`PaneToken`]: crate::pane_token::PaneToken
    pub(in crate::interactor) minter: PaneTokenMinter,
    /// Per-process cache of `repo_root -> origin URL` (the `Option` is stored
    /// faithfully so a missing origin is memoised too). An origin URL is a
    /// property of the on-disk repo and effectively never changes for the
    /// server's lifetime, so the cost of shelling out to `git config` for it
    /// is paid once per root. Tokio mutex because the lookup happens inside
    /// async code and may briefly hold the lock across an await point on a
    /// fresh miss.
    pub(in crate::interactor) repository_origin_cache:
        tokio::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
    /// The `gh` CLI driver. Held as a trait object (not generic) because the
    /// PR tab is the only consumer and it does not flow through the session
    /// actors — keeping it non-generic avoids threading a sixth type
    /// parameter through every interactor impl block.
    pub(in crate::interactor) gh_cli: Arc<dyn GhCli>,
    /// The external-tool opener used by `open cwd` (currently only VS Code
    /// via `code <path>`). Held as a trait object for the same reason as
    /// [`Self::gh_cli`]: it is not routed through the session actors, so a
    /// non-generic field keeps the interactor's five type parameters
    /// untouched.
    pub(in crate::interactor) external_opener: Arc<dyn ExternalOpener>,
    /// The registry of adapter factories, keyed by the provider each one
    /// drives ([`AgentAdapterFactory::provider`]). Held as trait objects for
    /// the same reason as [`Self::gh_cli`] — they are not routed through the
    /// session actors, so a non-generic field keeps the interactor's five type
    /// parameters untouched.
    ///
    /// Factories (rather than live adapters) are held because standing an
    /// adapter up may spawn a process — Codex spawns `codex app-server` and
    /// runs its `initialize` handshake; doing that at startup would break a
    /// machine without the provider installed. A factory carries only launch
    /// configuration, so construction has no side effects and the spawn is
    /// deferred to [`AgentAdapterFactory::connect`].
    ///
    /// A provider with a registered factory whose declared launch capability
    /// is adapter-backed dispatches its sessions over the adapter (see
    /// [`InteractorCore::adapter_backed_factory`]); every other provider takes
    /// the native PTY path. Empty when no factory has been wired (the default
    /// constructor, tests).
    pub(in crate::interactor) adapter_factories:
        HashMap<AgentProvider, Arc<dyn AgentAdapterFactory>>,
    /// Which launch options switch an agent's own safety mechanisms off, in that
    /// agent's vocabulary (see [`LaunchOptionDangerPolicy`]).
    ///
    /// Held as one trait object rather than per-provider, and not on the adapter
    /// factories, because Claude registers no factory at all while being the
    /// provider with the loudest dangerous flag. Defaults to
    /// [`NoDangerousLaunchOptions`]; production wiring installs the gateway-backed
    /// policy through [`Interactor::with_launch_option_danger_policy`].
    pub(in crate::interactor) launch_option_danger: Arc<dyn LaunchOptionDangerPolicy>,
    /// Resolves whether a provider's launch binary is present on this host, for
    /// the `/api/providers` availability endpoint. Held as a trait object for
    /// the same reason as [`Self::gh_cli`] — it is not routed through the
    /// session actors, so a non-generic field keeps the interactor's five type
    /// parameters untouched. The default constructor wires a stub reporting
    /// every binary as absent; production wiring installs the real PATH probe.
    pub(in crate::interactor) binary_detector: Arc<dyn BinaryDetector>,
    /// The Codex launch binary this server would spawn (`codex` by default,
    /// overridden by `DELTA_CODEX_BIN`). Stored so the availability endpoint
    /// probes the *same* binary a Codex spawn would use, rather than a divergent
    /// hardcoded path. Sourced from the same value handed to the Codex adapter
    /// factory at the composition root. The Claude launch binary is not
    /// duplicated here — it already lives on [`Self::launch`] as `claude_bin`.
    pub(in crate::interactor) codex_bin: String,
    /// The async event-emission seam: the sending half a producer that emits
    /// events *after* its driving call returned pushes on (see
    /// [`AsyncEventSink`]). The server owns the matching receiver and forwards
    /// each drained event to its broadcast.
    ///
    /// `None` by default — the synchronous return path (every hook handler and
    /// tick returning its `Vec<SessionEvent>`) is untouched, and a
    /// configuration that never wires the seam (the default constructor, the
    /// domain tests) simply drops any async emit. Production wiring installs the
    /// sink through [`Interactor::with_event_sink`]. Currently a dormant seam:
    /// no live path emits on it yet — the push-based producer (the Codex event
    /// pump) that does lands in a later change.
    pub(in crate::interactor) event_sink: Option<AsyncEventSink>,
    /// Per-lens memo of the latest PR search result, keeping a focus
    /// flip between the two lenses cheap. Bounded by
    /// [`PR_SEARCH_CACHE_TTL`] so the picker still picks up newly-opened
    /// PRs on the user's timescale.
    pub(in crate::interactor) pr_search_cache:
        tokio::sync::Mutex<std::collections::HashMap<PullRequestLens, PrSearchCacheEntry>>,
    /// request-row id → owning session, so a permission decision (which only
    /// carries the request id) can be routed to the right actor. Entries are
    /// claimed atomically by `decide_permission`/`abandon_permission_decision`,
    /// mirroring the waiter lifecycle inside the actor.
    ///
    /// Held on the core (not the outer [`Interactor`]) so both the routing layer
    /// and a session actor can reach it: the Claude path seeds it from the
    /// routing layer after the `PermissionRequest` hook returns, while the Codex
    /// event pump — which allocates the permission row inside the actor — seeds
    /// it there through the actor's [`Deref`](std::ops::Deref) to the core.
    pub(in crate::interactor) permission_index: std::sync::Mutex<HashMap<i64, SessionId>>,
    /// The repository clones in flight in this process, keyed by destination
    /// path, so a second request for a destination already being cloned joins
    /// the running job instead of starting a second `gh` process on the same
    /// directory.
    ///
    /// Deliberately in-memory, and deliberately unrelated to sessions: no
    /// session path reads it, so a clone in flight can never delay a spawn, and
    /// a restart simply forgets the job (see the `clone_repository` module for
    /// what that costs and how the leftovers are cleaned up).
    pub(in crate::interactor) clone_jobs: repository::CloneJobs,
    /// Test seam: how many background launch preparations are still running.
    ///
    /// A new-session send is answered before its launch has done anything, and
    /// the launch's progress is not legible from session state alone — the
    /// pending spawn is recorded partway through (just before the pane is
    /// created), and the launch's first hook may consume it before the task even
    /// reports back. So a test that asserts on what the launch *did* waits on
    /// this counter instead: incremented as each task is spawned, decremented
    /// once it has posted its `LaunchFinished`. See `Interactor::await_launch`.
    #[cfg(test)]
    pub(in crate::interactor) launches_in_flight: std::sync::atomic::AtomicUsize,
}

/// The public entry point: wraps the shared [`InteractorCore`] and routes
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
            launch_option_danger: Arc::new(NoDangerousLaunchOptions),
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

    /// Register the factory that lazily builds a provider's [`AgentAdapter`],
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

    /// Inject the [`LaunchOptionDangerPolicy`] that says which launch options
    /// disable an agent's own safety mechanisms.
    ///
    /// The default constructor wires [`NoDangerousLaunchOptions`], so a
    /// configuration that has not injected the real policy (the domain tests, dev
    /// harnesses) classifies nothing as dangerous and behaves as it did before
    /// the rule existed. Production wiring installs the gateway-backed policy.
    /// Same constraint as [`Self::with_launch_config`]: must run before any
    /// session actor is spawned.
    pub fn with_launch_option_danger_policy(
        self,
        policy: Arc<dyn LaunchOptionDangerPolicy>,
    ) -> Self {
        let Ok(mut core) = Arc::try_unwrap(self.core) else {
            panic!(
                "with_launch_option_danger_policy must be called before any session actor is spawned"
            );
        };
        core.launch_option_danger = policy;
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

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// How long the `PermissionRequest` hook response may block waiting for a
    /// browser decision before falling back to the TUI prompt.
    pub fn permission_decision_deadline(&self) -> std::time::Duration {
        self.launch.permission_decision_deadline
    }

    /// The registered adapter factory for `provider`, when that provider's
    /// sessions are adapter-backed — i.e. driven through an
    /// [`crate::agent::AgentAdapter`] binding (terminal-less) rather than the
    /// native PTY path.
    ///
    /// This is the single provider-dispatch predicate the session paths use:
    /// `Some` iff a factory is registered for the provider *and* its declared
    /// launch capability is adapter-backed (see
    /// [`crate::agent::LaunchCapability::is_adapter_backed`]). `None` sends
    /// the session down the native pane path (Claude, which never registers a
    /// factory) — so a new adapter-backed provider is dispatched correctly by
    /// registering its factory, with no new provider-identity `match` arm in
    /// the session paths.
    pub(in crate::interactor) fn adapter_backed_factory(
        &self,
        provider: AgentProvider,
    ) -> Option<Arc<dyn AgentAdapterFactory>> {
        self.adapter_factories
            .get(&provider)
            .filter(|factory| factory.capabilities().is_adapter_backed())
            .cloned()
    }

    /// Emit an event onto the async event seam, if one is wired.
    ///
    /// The complement of the synchronous return path: where a hook handler or
    /// tick hands its `Vec<SessionEvent>` back to a caller that broadcasts them,
    /// this pushes a single event onto the [`AsyncEventSink`] the server drains
    /// — for a producer that emits after its driving call has already returned.
    /// A no-op when no sink is wired (the default), so it is always safe to
    /// call. Currently dormant: the push-based producer that emits through it
    /// (the Codex event pump) lands in a later change.
    pub fn emit_async_event(&self, event: SessionEvent) {
        if let Some(sink) = &self.event_sink {
            sink.emit(event);
        }
    }
}

#[cfg(test)]
mod interactor_tests {
    use delta_model::AgentProvider;

    use crate::interactor::testing::{interactor, FakeAgentFactory};

    /// The provider-dispatch predicate resolves through the factory registry:
    /// a provider with a registered, adapter-backed factory gets it back; a
    /// provider with no factory (Claude — the native pane path) gets `None`.
    #[test]
    fn adapter_backed_factory_resolves_only_registered_providers() {
        let ix = interactor().with_adapter_factory(FakeAgentFactory::new("thr_1", None));

        let codex = ix.adapter_backed_factory(AgentProvider::Codex);
        assert!(
            codex.is_some_and(|f| f.provider() == AgentProvider::Codex),
            "the registered Codex factory is resolved by its provider key"
        );
        assert!(
            ix.adapter_backed_factory(AgentProvider::Claude).is_none(),
            "Claude registers no factory, so it stays on the native pane path"
        );
    }

    /// An unwired configuration (the default constructor) reports every
    /// provider as not adapter-backed, so no session is sent down an adapter
    /// path that cannot connect.
    #[test]
    fn adapter_backed_factory_is_none_when_nothing_is_wired() {
        let ix = interactor();
        assert!(ix.adapter_backed_factory(AgentProvider::Codex).is_none());
        assert!(ix.adapter_backed_factory(AgentProvider::Claude).is_none());
    }
}
