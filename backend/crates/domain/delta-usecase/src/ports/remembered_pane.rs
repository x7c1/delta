//! The tmux pane a Claude session's row remembers, so a restarted Delta can
//! find the session again.

/// Where a pane-backed (Claude) session's agent is running, recorded on the
/// session row while the session is bound and cleared when it closes.
///
/// A Claude Code process lives in a tmux pane on Delta's own tmux server, and
/// that pane outlives the Delta process: quitting, upgrading or crashing Delta
/// leaves it running. Everything else about an open session is runtime state
/// that a restart forgets, so this record is what lets the next process
/// re-adopt the pane instead of resuming the conversation into a second one
/// (see [`crate::Interactor::readopt_surviving_sessions`]).
///
/// Written by [`SessionStore::remember_pane`] when a pane is bound (a fresh
/// spawn's first hook, a resume, or a re-adoption) and erased by
/// [`SessionStore::forget_pane`] when the session is torn down. A row with no
/// record is a session no pane is known to be running for.
///
/// [`SessionStore::remember_pane`]: crate::ports::SessionStore::remember_pane
/// [`SessionStore::forget_pane`]: crate::ports::SessionStore::forget_pane
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RememberedPane {
    /// The Delta-minted tmux session name (`delta-<n>`) the agent runs in —
    /// what `tmux has-session` and `kill-session` are addressed by.
    pub tmux_session: String,
    /// The fully-qualified pane keystrokes go to and the terminal attaches to
    /// (`<tmux_session>:0.0`).
    pub pane: String,
    /// Whether the agent in this pane was launched with hook URLs that no
    /// longer reach Delta: the session was re-adopted by a process whose hook
    /// endpoint (port or secret) differs from the one the agent was started
    /// with. Such a session can still be watched and typed into through its
    /// terminal, but none of its hooks arrive, so the browser tells the user
    /// to close it and send again, which resumes it with current settings.
    ///
    /// Persisted rather than kept in memory because it describes the running
    /// agent, not this Delta process: a second restart whose endpoint happens
    /// to match the previous run's does not make the old URLs reach it again.
    pub hooks_unreachable: bool,
}
