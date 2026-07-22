//! [`OutstandingSend`]: the attribution-relevant view of one dispatched send.

use delta_model::{MessageUuid, Send, ThreadId};

/// The attribution-relevant view of one outstanding `dispatched` send: the
/// thread (and optional branch parent) its echo line must be attributed to,
/// and the text the echo is recognized by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutstandingSend {
    /// The send row id, echoed back in [`Effect::SendMatched`].
    pub id: i64,
    /// The thread this send is attributed to.
    pub thread_id: ThreadId,
    /// When branching, the message this reply is `to:`.
    pub semantic_parent_uuid: Option<MessageUuid>,
    /// The dispatched prompt text; the echo is matched by trimmed equality.
    pub text: String,
    /// The background-task identifier learned for the matching subagent launch,
    /// when one has been observed. Unused for human-prompt echo matching (which
    /// is text-based), present so a single struct can also carry the task-id
    /// correlation used to finish a background subagent when its
    /// `<task-notification>` is dropping the `<tool-use-id>` element.
    pub task_id: Option<String>,
}

impl From<&Send> for OutstandingSend {
    fn from(send: &Send) -> Self {
        Self {
            id: send.id,
            thread_id: send.thread_id,
            semantic_parent_uuid: send.semantic_parent_uuid.clone(),
            text: send.text.clone(),
            // The `Send` row has no background-task identifier of its own;
            // task ids are minted per subagent launch and learned later via the
            // `PostToolUse(Agent)` hook (see `RunningSubagent::task_id`).
            task_id: None,
        }
    }
}
