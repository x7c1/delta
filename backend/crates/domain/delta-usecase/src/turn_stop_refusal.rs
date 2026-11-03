//! Why a turn stop was refused.

/// Why [`Error::TurnNotStoppable`](crate::Error::TurnNotStoppable) refused to
/// stop a session's turn.
#[derive(Debug, Clone, Copy)]
pub enum TurnStopRefusal {
    /// No turn is running: there is nothing to stop.
    Idle,
    /// A dispatched send's echo is still awaited. That state has its own exits
    /// (cancelling the dispatched send, which injects the same Escape, and the
    /// echo-deadline watchdog), and a stop there would race the echo.
    AwaitingEcho,
    /// The session has no live pane (closed, ended, or still starting), so no
    /// turn is running in it.
    NotOpen,
    /// The session is terminal-less (Codex): it is stopped through its
    /// provider's own interrupt.
    NoTerminal,
}

impl std::fmt::Display for TurnStopRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Idle => "no turn is in progress",
            Self::AwaitingEcho => "a dispatched send is still awaiting its echo; cancel it instead",
            Self::NotOpen => "the session is not open",
            Self::NoTerminal => "the session has no terminal; use interrupt instead",
        })
    }
}
