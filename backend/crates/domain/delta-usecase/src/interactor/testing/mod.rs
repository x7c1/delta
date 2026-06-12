//! Shared `#[cfg(test)]` fakes and builders for the interactor use-case tests.
//!
//! Split so no single file becomes a dumping ground: the four port fakes each
//! get their own file, with the transcript-line builders, hook builders, send
//! targets, and the interactor factory grouped by purpose.

mod factory;
mod fake_store;
mod fake_tmux;
mod fake_transcript;
mod fake_workspace;
mod hooks;
mod targets;
mod transcript_lines;

pub(crate) use factory::{
    interactor, interactor_with_failing_create_session, interactor_with_failing_tmux,
    TEST_SETTINGS_JSON, TEST_SETTINGS_PATH, TEST_WORKDIR_BASE,
};
pub(crate) use fake_store::FakeStore;
pub(crate) use fake_tmux::FakeTmux;
pub(crate) use fake_transcript::FakeTranscript;
pub(crate) use fake_workspace::FakeWorkspace;
pub(crate) use hooks::{session_start, submit, submit_for, submit_in};
pub(crate) use targets::{branch_off, to};
pub(crate) use transcript_lines::{
    assistant_line, assistant_line_at, interrupt_line, queued_command_line, tool_result_line,
    user_line,
};

// Re-exported so test files can call `SessionStore` methods on `ix.store()`
// (e.g. `main_thread_id`, `create_thread`) via the `testing::*` glob without
// each importing the trait directly.
pub(crate) use crate::ports::SessionStore;
