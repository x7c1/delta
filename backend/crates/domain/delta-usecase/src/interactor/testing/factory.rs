//! Construct the test interactor over the in-memory fakes, plus the accessors
//! tests use to reach into the fakes the interactor owns.

use std::sync::Arc;

use crate::agent::AgentAdapterFactory;
use crate::Interactor;

use super::{
    FakeAgentFactory, FakeGhCli, FakeGitWorktree, FakeStore, FakeTmux, FakeTranscript,
    FakeWorkspace,
};

/// The base working directory the test interactor spawns sessions under.
pub(crate) const TEST_WORKDIR_BASE: &str = "/work";

/// The neutral base directory the test interactor places per-session git
/// worktrees under. Deliberately distinct from [`TEST_WORKDIR_BASE`] so a test
/// can assert a worktree lands under `worktree_base` while a default spawn still
/// lands under `session_workdir_base`.
pub(crate) const TEST_WORKTREE_BASE: &str = "/worktrees";

/// The settings JSON the test interactor writes for each launch.
pub(crate) const TEST_SETTINGS_JSON: &str = r#"{"hooks":{}}"#;

/// The Delta-owned path the test interactor writes settings to and passes via
/// `claude --settings`. Outside any spawn workdir, on purpose.
pub(crate) const TEST_SETTINGS_PATH: &str = "/run/delta/settings.json";

/// The concrete interactor type the use-case tests build over the in-memory
/// fakes.
pub(crate) type TestInteractor =
    Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace, FakeGitWorktree>;

pub(crate) fn interactor() -> TestInteractor {
    interactor_with_git(FakeGitWorktree::default())
}

/// Build a test interactor with a specific [`FakeGitWorktree`], for the
/// worktree spawn-wiring tests; everything else is the default fake.
pub(crate) fn interactor_with_git(git_worktree: FakeGitWorktree) -> TestInteractor {
    interactor_with_git_and_worktree_base(git_worktree, TEST_WORKTREE_BASE)
}

/// Like [`interactor_with_git`] but lets the caller override the
/// `worktree_base` the interactor is wired with. Used by the Repository tab
/// tests that need real existing paths under `worktree_base` (so the
/// session-derived clone rows survive the lazy-GC filter) — point this at a
/// `tempfile::tempdir()` and create the child directories under it.
pub(crate) fn interactor_with_git_and_worktree_base(
    git_worktree: FakeGitWorktree,
    worktree_base: impl Into<String>,
) -> TestInteractor {
    Interactor::new(
        FakeTmux::default(),
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        git_worktree,
        TEST_WORKDIR_BASE,
        worktree_base,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    )
}

/// Build a test interactor with both a specific [`FakeGitWorktree`] and a
/// shared [`FakeGhCli`], for the PR-tab use-case tests that need to
/// script both the local-clone registry (via the store + gateway-resolved
/// origin URLs) and the gh CLI's answers — and reach back into the gh
/// fake afterwards to inspect what the use case shelled out.
pub(crate) fn interactor_with_git_and_gh(
    git_worktree: FakeGitWorktree,
    gh_cli: Arc<FakeGhCli>,
) -> TestInteractor {
    interactor_with_git(git_worktree).with_gh_cli(gh_cli as Arc<dyn crate::ports::GhCli>)
}

/// Build a test interactor with a Codex [`AgentAdapterFactory`] wired in, for
/// the terminal-less Codex session-creation tests. Everything else is the
/// default fake set.
pub(crate) fn interactor_with_codex_factory(factory: Arc<FakeAgentFactory>) -> TestInteractor {
    interactor().with_codex_adapter_factory(factory as Arc<dyn AgentAdapterFactory>)
}

/// An interactor whose tmux dispatch always fails.
pub(crate) fn interactor_with_failing_tmux() -> TestInteractor {
    Interactor::new(
        FakeTmux {
            fail: true,
            ..Default::default()
        },
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        FakeGitWorktree::default(),
        TEST_WORKDIR_BASE,
        TEST_WORKTREE_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    )
}

/// An interactor whose tmux session launch (`create_session`) always fails.
pub(crate) fn interactor_with_failing_create_session() -> TestInteractor {
    Interactor::new(
        FakeTmux {
            fail_create: true,
            ..Default::default()
        },
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        FakeGitWorktree::default(),
        TEST_WORKDIR_BASE,
        TEST_WORKTREE_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    )
}

/// The transcript path the seed-session test hooks (`submit`/`session_start`)
/// install on the session row. Tests that fire `PreToolUse` / `PostToolUse` /
/// `PermissionRequest` against the seeded session use this same path so the
/// hook is recognised as belonging to the parent session (and not filtered
/// out as a nested subagent's hook by the transcript-path guard).
pub(crate) const SEED_TRANSCRIPT_PATH: &str = "/tmp/t.jsonl";

// Helper accessors used only in tests to reach into the fakes the interactor owns.
impl TestInteractor {
    /// Register `sess-1` as an open, ready, idle session.
    ///
    /// Fires the first `UserPromptSubmit` (which registers the session) and then
    /// a `Stop`, so the registration turn completes and `turn_active` is clear.
    /// A bare `UserPromptSubmit` marks the turn in flight, so tests that go on to
    /// dispatch a branch/quoted send must start from an idle session — otherwise
    /// that send would be queued behind the still-open registration turn.
    ///
    /// It also binds a live, ready pane for `sess-1`, so a following send
    /// dispatches immediately on the normal path rather than resuming the session
    /// (which, under the readiness gate, would hold the first keystroke). The
    /// resume gate has its own focused tests; the defer/enqueue tests want a
    /// plain open session.
    pub(crate) async fn seed_session(&self) {
        self.on_user_prompt_submit(super::submit("seed"))
            .await
            .unwrap();
        self.on_stop(crate::ports::StopHook {
            session_id: delta_model::SessionId::from("sess-1"),
            stop_reason: None,
        })
        .await
        .unwrap();
        self.bind_open_session("delta-seed", &delta_model::SessionId::from("sess-1"))
            .await;
    }

    pub(crate) fn transcript_fake(&self) -> &FakeTranscript {
        self.transcript()
    }

    pub(crate) fn tmux_fake(&self) -> &FakeTmux {
        self.tmux()
    }

    pub(crate) fn workspace_fake(&self) -> &FakeWorkspace {
        self.workspace()
    }

    pub(crate) fn git_worktree_fake(&self) -> &FakeGitWorktree {
        self.git_worktree()
    }
}
