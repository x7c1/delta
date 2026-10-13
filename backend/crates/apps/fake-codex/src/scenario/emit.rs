//! One scripted emission played during a turn.

use serde::Deserialize;
use serde_json::Value;

use super::DEFAULT_APPROVAL_METHOD;

/// One scripted emission played during a turn. See the `scenario` module docs.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Emit {
    ItemStarted {
        item: Value,
    },
    ItemCompleted {
        item: Value,
    },
    /// A streaming fragment of an assistant message: emits the real
    /// `item/agentMessage/delta` notification (`{ itemId, delta, turnId }`).
    AgentMessageDelta {
        item_id: String,
        delta: String,
    },
    TurnStarted,
    TurnCompleted {
        status: String,
    },
    RequestApproval {
        #[serde(default = "default_approval_method")]
        method: String,
        #[serde(default)]
        params: Value,
        /// When true, the fake **suspends** the turn after emitting this approval
        /// and resumes only once the client answers it — so a scenario can gate a
        /// turn on a real decision, and the fake, on resuming, echoes the
        /// decision string it received as an assistant message (observable proof
        /// the decision round-tripped, and the only way a test can see *which*
        /// value went over the wire). Default false keeps the fire-and-forget
        /// behavior (emit and continue without waiting).
        #[serde(default)]
        blocking: bool,
    },
    /// Suspend the turn until **every** approval emitted so far has been
    /// answered, then play the rest of the turn.
    ///
    /// This is the parallel-approval gate: a scenario emits N non-blocking
    /// `request_approval` steps (so all N are outstanding at once, as a real
    /// `codex app-server` does when the model fans tool calls out) and then parks
    /// the turn here. Each answer is echoed as an assistant message as it
    /// arrives, so a test can prove every request round-tripped — and the turn
    /// completes only after the last one, which is what makes an unanswerable
    /// request show up as a hung turn rather than a silent pass.
    ///
    /// A no-op when no approval is outstanding.
    AwaitApprovals,
    /// Die where a real app-server would: the fake exits at this point in the
    /// turn, closing its stdout, so the client sees the EOF a killed or crashed
    /// `codex app-server` produces.
    ///
    /// This is how a scenario re-enacts the field failure — a process that goes
    /// away mid-turn, with approvals outstanding and a turn still in flight —
    /// deterministically, without a test having to find and signal a PID. Nothing
    /// scripted after it ever plays.
    Exit,
    Notification {
        method: String,
        #[serde(default)]
        params: Value,
    },
    /// An account-scoped notification, emitted with its params **verbatim** —
    /// no `threadId` is stamped in. See the `scenario` module docs for why the
    /// distinction from [`Emit::Notification`] is load-bearing.
    AccountNotification {
        method: String,
        #[serde(default)]
        params: Value,
    },
}

fn default_approval_method() -> String {
    DEFAULT_APPROVAL_METHOD.to_owned()
}
