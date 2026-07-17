//! Shared `#[cfg(test)]` fakes and builders for the interactor use-case tests.
//!
//! Split so no single file becomes a dumping ground: the four port fakes each
//! get their own file, with the transcript-line builders, hook builders, send
//! targets, and the interactor factory grouped by purpose.

mod factory;
mod fake_agent;
mod fake_binary_detector;
mod fake_external_opener;
mod fake_gh_cli;
mod fake_git_worktree;
mod fake_store;
mod fake_tmux;
mod fake_transcript;
mod fake_workspace;
mod hooks;
mod targets;
mod transcript_lines;

pub(crate) use factory::{
    interactor, interactor_with_codex_factory, interactor_with_failing_create_session,
    interactor_with_failing_tmux, interactor_with_git, interactor_with_git_and_gh,
    interactor_with_git_and_worktree_base, TestInteractor, SEED_TRANSCRIPT_PATH,
    TEST_SETTINGS_JSON, TEST_SETTINGS_PATH, TEST_WORKDIR_BASE, TEST_WORKTREE_BASE,
};
pub(crate) use fake_agent::FakeAgentFactory;
pub(crate) use fake_binary_detector::FakeBinaryDetector;
pub(crate) use fake_external_opener::FakeExternalOpener;
pub(crate) use fake_gh_cli::FakeGhCli;
pub(crate) use fake_git_worktree::FakeGitWorktree;
pub(crate) use fake_store::FakeStore;
pub(crate) use fake_tmux::FakeTmux;
pub(crate) use fake_transcript::FakeTranscript;
pub(crate) use fake_workspace::FakeWorkspace;
pub(crate) use hooks::{session_start, submit, submit_for, submit_in};
pub(crate) use targets::{branch_off, to};
pub(crate) use transcript_lines::{
    agent_tool_use_line, api_error_line, assistant_line, assistant_line_at,
    background_tool_use_line, bash_tool_use_line, compact_summary_line, errored_tool_result_line,
    foreground_agent_tool_use_line, interrupt_line, local_command_caveat_line,
    local_command_name_line, local_command_stdout_line, queued_command_line, queued_replay_line,
    task_notification_line, task_notification_line_both_missing,
    task_notification_line_task_id_only, tool_result_line, user_line, with_prompt_id,
};

// Re-exported so test files can call `SessionStore` methods on `ix.store()`
// (e.g. `main_thread_id`, `create_thread`) via the `testing::*` glob without
// each importing the trait directly.
pub(crate) use crate::ports::SessionStore;
