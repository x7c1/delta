//! The turn state of one session.

/// The turn state of one session. See the `turn` module docs for the lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TurnState {
    /// No turn in flight; queued sends may dispatch.
    #[default]
    Idle,
    /// A send was dispatched and its `UserPromptSubmit` echo is awaited.
    ///
    /// `slash_command` records whether the send's text is a slash command
    /// ([`claude_format::is_slash_command_send`]): a local command such as
    /// `/cost` never echoes, so for such a send the echo deadline is the end
    /// of its turn rather than a sign its keystrokes were lost.
    ///
    /// [`claude_format::is_slash_command_send`]: delta_attribution::claude_format::is_slash_command_send
    AwaitingEcho { send_id: i64, slash_command: bool },
    /// A turn is running: a matched Delta send, or external pane input (`None`).
    InFlight { send_id: Option<i64> },
}
