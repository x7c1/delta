//! The [`Interactor`]: orchestrates the ports into Delta's use cases.
//!
//! The use cases are split by area into child modules (`runtime`, `lifecycle`,
//! `enqueue`, `hooks`, `sync`, `listing`, `context`, `workdir`), each carrying
//! its own `impl Interactor` block. The struct itself stays defined here with
//! its fields private; child modules reach those fields as ancestor-private
//! state without any visibility widening.

mod context;
mod enqueue;
mod hooks;
mod lifecycle;
mod listing;
mod permission_decision;
mod runtime;
mod sync;
mod turn_input;
mod workdir;

pub use hooks::PermissionWait;
pub use permission_decision::PermissionDecision;

#[cfg(test)]
mod testing;

use crate::launch_config::LaunchConfig;
use crate::open_sessions::OpenSessions;
use crate::pane_token::PaneTokenMinter;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::TurnRegistry;

/// Holds the injected capabilities and exposes Delta's use cases.
///
/// Generic over the four ports so callers can inject any implementation. The
/// composition root and the application share a single concrete type through
/// the [`BoxedInteractor`] alias, which erases the gateways behind trait
/// objects; this keeps the transport layer's shared state non-generic while
/// still allowing tests to substitute fakes.
pub struct Interactor<T, X, S, W> {
    tmux: T,
    transcript: X,
    store: S,
    workspace: W,
    /// Base directory for per-spawn working directories.
    ///
    /// Each fresh spawn runs in its own `<base>/<token>` subdirectory. The
    /// workdir is no longer the hook-binding key — correlation is by the
    /// Delta-minted session id pinned via `claude --session-id` — so this base
    /// is free to become a user-selected project path in a later change without
    /// breaking spawn↔session correlation.
    session_workdir_base: String,
    /// The Claude Code settings JSON whose hooks point back at this server (and
    /// which pins the session theme). Rendered by the caller (with the running
    /// port) and held verbatim; written to [`Self::session_settings_path`] and
    /// passed to `claude --settings`.
    session_settings_json: String,
    /// Delta-owned path the settings JSON is written to before each launch, then
    /// passed to `claude --settings <path>`. Kept *outside* any session working
    /// directory so spawning/resuming in a real project never overwrites that
    /// project's own `.claude/settings.json`.
    session_settings_path: String,
    /// How sessions are launched (which binary) and how long the watchdog
    /// waits on a launch. Production defaults via [`LaunchConfig::default`];
    /// overridden through [`Self::with_launch_config`].
    launch: LaunchConfig,
    /// Mints unique [`PaneToken`]s for fresh spawns.
    ///
    /// [`PaneToken`]: crate::pane_token::PaneToken
    minter: PaneTokenMinter,
    /// The in-memory registry of live (open) panes. Rebuilt empty on boot, so
    /// open/closed is process-runtime state and never persisted.
    open_sessions: tokio::sync::Mutex<OpenSessions>,
    /// The per-session turn state machine. Like [`Self::open_sessions`] this is
    /// process-runtime state rebuilt empty on boot: after a restart every
    /// session is closed (the registry above is empty), and a closed session
    /// has no turn in flight, so absence — which reads as `Idle` — is exactly
    /// right. See the `turn` module docs.
    turns: tokio::sync::Mutex<TurnRegistry>,
    /// Oneshot waiters for permission requests the browser may decide, keyed
    /// by request-row id. Registered by `on_permission_request` (whose hook
    /// response blocks on the receiver), resolved by `decide_permission`, and
    /// abandoned on the transport's timeout. Runtime-only by nature: a waiter
    /// is meaningful only while its hook request is in flight.
    pending_permissions: tokio::sync::Mutex<
        std::collections::HashMap<i64, tokio::sync::oneshot::Sender<PermissionDecision>>,
    >,
    /// Serializes [`Self::sync_transcript`] across callers.
    ///
    /// Both the hook handlers and the background transcript tail can sync
    /// concurrently. The read-cursor → read-file → ingest → set-cursor sequence
    /// is not atomic, so without this lock two interleaved syncs could read the
    /// same lines from the same starting cursor and double-ingest, or race the
    /// cursor write. Holding this for the whole sequence makes ingestion serial.
    sync_lock: tokio::sync::Mutex<()>,
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
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
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
        Self {
            tmux,
            transcript,
            store,
            workspace,
            session_workdir_base: session_workdir_base.into(),
            session_settings_json: session_settings_json.into(),
            session_settings_path: session_settings_path.into(),
            launch: LaunchConfig::default(),
            minter: PaneTokenMinter::new(),
            open_sessions: tokio::sync::Mutex::new(OpenSessions::default()),
            turns: tokio::sync::Mutex::new(TurnRegistry::default()),
            pending_permissions: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            sync_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// How long the `PermissionRequest` hook response may block waiting for a
    /// browser decision before falling back to the TUI prompt.
    pub fn permission_decision_deadline(&self) -> std::time::Duration {
        self.launch.permission_decision_deadline
    }

    /// Replace the launch configuration (binary to spawn, watchdog deadlines).
    ///
    /// A builder-style override so the many existing constructor call sites
    /// keep the production defaults without naming them; the composition root
    /// applies whatever the environment configured.
    pub fn with_launch_config(mut self, launch: LaunchConfig) -> Self {
        self.launch = launch;
        self
    }
}
