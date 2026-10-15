//! How this run's hook endpoint relates to the previous run's.

/// How the hook endpoint this run serves — the port and secret in the hook
/// URLs — relates to the one the previous run served, as boot-time
/// re-adoption needs to know it.
///
/// A Claude Code session that survived the restart in tmux keeps calling the
/// URLs it was launched with, so what re-adoption does with its pane depends
/// on where those calls now land.
///
/// Ordered by how much a surviving session is affected, so two findings about
/// the same start — the secret's and the port's — combine with
/// [`Ord::max`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HookEndpointChange {
    /// The same port and secret: a surviving agent's hooks reach this server.
    Unchanged,
    /// The port is the same but the secret differs (or there was no previous
    /// run to compare with). A surviving agent's hooks reach this server and
    /// are refused, so its session is re-adopted and marked as unable to
    /// deliver hooks.
    Changed,
    /// The server could not get its previous port back and listens on another
    /// one. A surviving agent's hooks go to the old port, where some other
    /// process may now listen and would receive what they carry — the hook
    /// secret and the session's prompts and tool calls. So its session is not
    /// kept open: its pane is ended and the session closed with a reason, and
    /// resuming it relaunches the agent with hooks aimed at this port.
    PortMoved,
}
