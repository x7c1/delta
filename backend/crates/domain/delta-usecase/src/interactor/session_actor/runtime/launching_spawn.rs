//! The accept→launch window: the state an actor holds between answering a
//! new-session send and its background launch preparation reporting back,
//! with the two transitions that open and close that window.

use std::time::Instant;

use delta_model::{AgentProvider, ThreadId};

use crate::agent::LaunchOptionSpec;
use crate::pane_token::PaneToken;

use super::{PlannedWorktree, SessionRuntime};

/// An accepted session whose launch preparation is still running.
///
/// `POST /api/sends { new_session: true }` answers as soon as the session is
/// *accepted*: the row exists (listed as `spawning`), the first send is
/// recorded, and the response carries real ids. Everything expensive — the
/// worktree build, and then whatever standing the agent up costs for the chosen
/// provider (Claude: the trust seed, the settings file and `tmux new-session`;
/// an adapter-backed provider: `connect` plus `thread/start`) — happens
/// afterwards on a background task, which reports back through this actor's own
/// mailbox. This entry is what the actor holds in between: no agent exists yet,
/// so nothing can bind, but the session is emphatically not idle.
///
/// Both providers use this same window, so the accept→launch split is one
/// mechanism rather than two: what differs is only the per-provider tail
/// ([`LaunchTarget`]) and which checkpoint message the task posts back
/// ([`SessionInput::LaunchPrepared`] for a pane,
/// [`SessionInput::AdapterLaunchPrepared`] for an adapter).
///
/// For a **pane** launch the entry lasts until the launch's last step: the task
/// checks in once everything but the pane is in place, and the handler swaps
/// this entry for the [`PendingSpawn`] the launch's first hook binds — *before*
/// the pane is created, so no hook can arrive ahead of that record. For an
/// **adapter** launch there is no hook and no pending spawn: the checkpoint
/// handler binds the live adapter directly, so the entry is taken and never
/// replaced. A preparation that fails before its checkpoint never reaches
/// either swap and is rolled back from here instead, on `LaunchFinished`.
///
/// Its presence makes [`SessionRuntime::has_live_pane`] true (a cold start must
/// not spawn a second session alongside it), keeps
/// [`SessionRuntime::is_empty`] false (the actor must stay alive to receive
/// the launch's outcome), and makes
/// [`SessionRuntime::is_launching_or_pending`] true (a plain send arriving now
/// is queued until the launch binds and a branch send is refused with
/// `session_spawning`, exactly as one arriving against a [`PendingSpawn`] is).
/// The watchdog drains deliberately do NOT see it: a
/// launch preparation has its own deadline on the task, and the bind deadline
/// only starts when the pane is about to exist.
///
/// [`SessionInput::LaunchPrepared`]: crate::interactor::session_actor::input::SessionInput::LaunchPrepared
/// [`SessionInput::AdapterLaunchPrepared`]: crate::interactor::session_actor::input::SessionInput::AdapterLaunchPrepared
/// [`PendingSpawn`]: super::PendingSpawn
#[derive(Debug, Clone)]
pub struct LaunchingSpawn {
    /// The launch's key: the Delta-minted tmux session name for a pane launch,
    /// and the session-derived, never-tmux-bound stand-in
    /// ([`PaneToken::for_adapter_launch`]) for an adapter launch. It is what
    /// pairs the task's reports with the entry they settle.
    pub token: PaneToken,
    /// The directory the agent will be launched in, as planned by the accept
    /// phase — the worktree path for a worktree spawn, the user-selected
    /// directory for a plain one, the per-token (Claude) or per-session
    /// (adapter) scratch dir otherwise. Also what the eager session row already
    /// stored as its `cwd`, which is why it is computed before the build rather
    /// than read back from it.
    pub workdir: String,
    /// The worktree still to build, when one was requested. `None` for a plain
    /// spawn, which has no git work left to do.
    pub worktree: Option<PlannedWorktree>,
    /// What the launch stands up once the worktree is in place — the one thing
    /// that differs between a pane-backed and an adapter-backed launch.
    pub target: LaunchTarget,
    /// When the send was accepted — the instant the REST response went out.
    ///
    /// Not a watchdog deadline (the launch task owns its own timeout): it is
    /// the stamp that lets the launch's log line report how long the
    /// preparation actually took, which is the number this whole split exists
    /// to keep out of the request.
    pub accepted_at: Instant,
}

/// The provider-specific tail of a launch: everything the background task still
/// has to do once the (shared) worktree build is done — and the only thing in
/// the launch machinery that branches on the provider.
#[derive(Debug, Clone)]
pub enum LaunchTarget {
    /// Claude: a tmux pane running `claude`, bound by the first hook it fires.
    Pane(PaneLaunch),
    /// An adapter-backed provider (Codex): a `connect` + `thread/start` over
    /// the provider's adapter, bound by the checkpoint the task posts back.
    Adapter(AdapterLaunch),
}

/// The tail of a pane-backed (Claude) launch.
#[derive(Debug, Clone)]
pub struct PaneLaunch {
    /// The pane keystrokes will be sent to (`<token>:0.0`) once it exists.
    pub pane: String,
    /// Whether Claude Code's workspace-trust dialog must be pre-accepted for
    /// [`LaunchingSpawn::workdir`] before launching there.
    pub seed_trust: bool,
    /// The full argv the launch runs (`claude --settings … --session-id …`,
    /// the user's launch options, then any first prompt).
    pub command: Vec<String>,
}

/// The tail of an adapter-backed (Codex) launch.
///
/// It carries what the connect/`thread/start` needs (the provider and the
/// user's launch options) plus what the checkpoint handler needs to finish the
/// session off on the actor: the `main` thread the eager row created, and the
/// first prompt's already-written `queued` send row.
#[derive(Debug, Clone)]
pub struct AdapterLaunch {
    /// Which adapter-backed provider to resolve through the factory registry.
    pub provider: AgentProvider,
    /// The user-selected launch options, already resolved to neutral
    /// `(name, value?)` pairs by the accept phase. Rendered for the provider by
    /// the adapter, never here.
    pub launch_options: Vec<LaunchOptionSpec>,
    /// The eager row's `main` thread, which the content source is folded onto
    /// and the first prompt's turn dispatches against.
    pub main_thread_id: ThreadId,
    /// The first prompt's `send` row, written `queued` by the accept phase.
    /// `None` for a prompt-less spawn. Nothing has received it until the
    /// adapter's `turn/start`, so it is promoted and dispatched by the
    /// checkpoint handler.
    pub first_send_id: Option<i64>,
}

impl SessionRuntime {
    /// Record an accepted session whose launch preparation is now running in
    /// the background.
    pub fn start_launching(&mut self, launching: LaunchingSpawn) {
        debug_assert!(
            self.launching_spawn.is_none() && self.pending_spawn.is_none(),
            "a session id is minted per spawn, so at most one launch is ever in flight"
        );
        self.launching_spawn = Some(launching);
    }

    /// Take the in-flight launch back out if it carries this token — the launch
    /// task reaching its `LaunchPrepared` checkpoint, or reporting a failure it
    /// hit before that.
    ///
    /// Keyed by token so a late report from a launch that was already rolled
    /// back (or, defensively, from some other launch) cannot consume an
    /// unrelated entry. `None` means there is nothing left to finish: the caller
    /// abandons the launch (at the checkpoint) or treats the report as stale.
    pub fn take_launching_for_token(&mut self, token: &PaneToken) -> Option<LaunchingSpawn> {
        if self
            .launching_spawn
            .as_ref()
            .is_some_and(|l| &l.token == token)
        {
            return self.launching_spawn.take();
        }
        None
    }

    /// The in-flight launch, for the test seams that read launch state back.
    #[cfg(test)]
    pub(crate) fn launching_spawn(&self) -> Option<&LaunchingSpawn> {
        self.launching_spawn.as_ref()
    }
}
