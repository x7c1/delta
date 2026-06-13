//! The [`Interactor`]: orchestrates the ports into Delta's use cases.
//!
//! The use cases are split by area into child modules (`runtime`, `lifecycle`,
//! `enqueue`, `hooks`, `sync`, `listing`, `context`, `workdir`), each carrying
//! its own `impl` block. The injected capabilities live in
//! [`InteractorCore`]; the [`Interactor`] wraps the core in an [`Arc`] and
//! derefs to it, keeping the core's read paths callable directly on the
//! interactor. Per-session runtime state lives in the `session_actor` module
//! (one actor task per session), reached through the `routing` impl.

mod context;
mod enqueue;
mod hooks;
mod lifecycle;
mod listing;
mod permission_decision;
mod routing;
mod runtime;
pub(crate) mod session_actor;
mod sync;
mod turn_input;
mod workdir;

pub use hooks::PermissionWait;
pub use permission_decision::PermissionDecision;
pub use session_actor::runtime::{PendingPermission, PendingQuestion, SessionLiveState};

#[cfg(test)]
mod testing;

use std::collections::HashMap;
use std::sync::Arc;

use delta_model::SessionId;

use crate::launch_config::LaunchConfig;
use crate::pane_token::PaneTokenMinter;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};

use session_actor::registry::SessionRegistry;

/// Holds the injected capabilities and implements Delta's use cases.
///
/// Generic over the four ports so callers can inject any implementation. This
/// is the shared core behind [`Interactor`]: the interactor (and any
/// background task it spawns) holds it through an [`Arc`], so the core itself
/// carries no registry of those tasks — only the capabilities and the
/// process-runtime state.
pub struct InteractorCore<T, X, S, W> {
    pub(in crate::interactor) tmux: T,
    pub(in crate::interactor) transcript: X,
    pub(in crate::interactor) store: S,
    pub(in crate::interactor) workspace: W,
    /// Base directory for per-spawn working directories.
    ///
    /// Each fresh spawn runs in its own `<base>/<token>` subdirectory. The
    /// workdir is no longer the hook-binding key — correlation is by the
    /// Delta-minted session id pinned via `claude --session-id` — so this base
    /// is free to become a user-selected project path in a later change without
    /// breaking spawn↔session correlation.
    pub(in crate::interactor) session_workdir_base: String,
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
pub struct Interactor<T, X, S, W> {
    core: Arc<InteractorCore<T, X, S, W>>,
    /// session_id → actor mailbox; actors spawn on first contact.
    pub(in crate::interactor) sessions: SessionRegistry<T, X, S, W>,
    /// request-row id → owning session, so a permission decision (which only
    /// carries the request id) can be routed to the right actor. Entries are
    /// claimed atomically by `decide_permission`/`abandon_permission_decision`,
    /// mirroring the waiter lifecycle inside the actor.
    pub(in crate::interactor) permission_index: std::sync::Mutex<HashMap<i64, SessionId>>,
}

impl<T, X, S, W> std::ops::Deref for Interactor<T, X, S, W> {
    type Target = InteractorCore<T, X, S, W>;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

/// An [`Interactor`] with its four ports type-erased behind trait objects.
///
/// Both the production composition root and integration tests build this exact
/// type, so the transport layer's shared state stays non-generic regardless of
/// which gateways are wired in.
pub type BoxedInteractor =
    Interactor<Box<dyn TmuxDriver>, Box<dyn Transcript>, Box<dyn SessionStore>, Box<dyn Workspace>>;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
{
    /// Construct an Interactor from the four injected ports plus the spawn
    /// configuration (the base working directory, the rendered settings JSON,
    /// and the Delta-owned path that JSON is written to for `--settings`).
    pub fn new(
        tmux: T,
        transcript: X,
        store: S,
        workspace: W,
        session_workdir_base: impl Into<String>,
        session_settings_json: impl Into<String>,
        session_settings_path: impl Into<String>,
    ) -> Self {
        let core = Arc::new(InteractorCore {
            tmux,
            transcript,
            store,
            workspace,
            session_workdir_base: session_workdir_base.into(),
            session_settings_json: session_settings_json.into(),
            session_settings_path: session_settings_path.into(),
            launch: LaunchConfig::default(),
            minter: PaneTokenMinter::new(),
        });
        let sessions = SessionRegistry::new(&core);
        Self {
            core,
            sessions,
            permission_index: std::sync::Mutex::new(HashMap::new()),
        }
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
        Self {
            core,
            sessions,
            permission_index: self.permission_index,
        }
    }
}

impl<T, X, S, W> InteractorCore<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// How long the `PermissionRequest` hook response may block waiting for a
    /// browser decision before falling back to the TUI prompt.
    pub fn permission_decision_deadline(&self) -> std::time::Duration {
        self.launch.permission_decision_deadline
    }
}
