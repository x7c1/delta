//! Enqueue use cases: route a user input to its session, write the pending
//! send, and dispatch the keystrokes.

mod deferred_pending_send;
mod enqueue_into_open;
mod enqueue_send;
mod ensure_open;
mod provisional_branch_title;

#[cfg(test)]
mod tests;

pub(in crate::interactor::enqueue) use deferred_pending_send::deferred_pending_send;
pub(in crate::interactor::enqueue) use provisional_branch_title::provisional_branch_title;
