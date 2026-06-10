//! Thread-switch `additionalContext` framing injected on `UserPromptSubmit`.

mod delimit_quote;
mod frame_branch_entry_context;
mod frame_locator_context;
mod frame_thread_switch_context;
mod thread_switch_context;

#[cfg(test)]
mod tests;

pub(in crate::interactor) use frame_branch_entry_context::frame_branch_entry_context;
pub(in crate::interactor) use frame_locator_context::frame_locator_context;
pub(in crate::interactor) use frame_thread_switch_context::frame_thread_switch_context;

pub(in crate::interactor::context) use delimit_quote::delimit_quote;
