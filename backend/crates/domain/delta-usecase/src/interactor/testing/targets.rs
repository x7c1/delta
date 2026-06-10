//! Builders for the [`SendTarget`]s the interactor tests enqueue.

use delta_model::{MessageUuid, ThreadId};

use crate::SendTarget;

/// A plain send into an existing thread.
pub(crate) fn to(thread_id: ThreadId) -> SendTarget {
    SendTarget::Thread {
        thread_id,
        branch_from: None,
    }
}

/// A branch send: the first message of a new branch off `parent`, hanging off
/// `thread_id` as the parent thread.
pub(crate) fn branch_off(thread_id: ThreadId, parent: &MessageUuid) -> SendTarget {
    SendTarget::Thread {
        thread_id,
        branch_from: Some(parent.clone()),
    }
}
