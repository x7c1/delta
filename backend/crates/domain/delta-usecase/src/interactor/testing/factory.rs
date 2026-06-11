//! Construct the test interactor over the in-memory fakes, plus the accessors
//! tests use to reach into the fakes the interactor owns.

use crate::Interactor;

use super::{FakeStore, FakeTmux, FakeTranscript, FakeWorkspace};

/// The base working directory the test interactor spawns sessions under.
pub(crate) const TEST_WORKDIR_BASE: &str = "/work";

/// The settings JSON the test interactor writes for each launch.
pub(crate) const TEST_SETTINGS_JSON: &str = r#"{"hooks":{}}"#;

/// The Delta-owned path the test interactor writes settings to and passes via
/// `claude --settings`. Outside any spawn workdir, on purpose.
pub(crate) const TEST_SETTINGS_PATH: &str = "/run/delta/settings.json";

pub(crate) fn interactor() -> Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace> {
    Interactor::new(
        FakeTmux::default(),
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        TEST_WORKDIR_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    )
}

/// An interactor whose tmux dispatch always fails.
pub(crate) fn interactor_with_failing_tmux(
) -> Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace> {
    Interactor::new(
        FakeTmux {
            fail: true,
            ..Default::default()
        },
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        TEST_WORKDIR_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    )
}

/// An interactor whose tmux session launch (`create_session`) always fails.
pub(crate) fn interactor_with_failing_create_session(
) -> Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace> {
    Interactor::new(
        FakeTmux {
            fail_create: true,
            ..Default::default()
        },
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        TEST_WORKDIR_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    )
}

// Helper accessors used only in tests to reach into the fakes the interactor owns.
impl Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace> {
    /// Register `sess-1` as an open, ready, idle session.
    ///
    /// Fires the first `UserPromptSubmit` (which registers the session) and then
    /// a `Stop`, so the registration turn completes and `turn_active` is clear.
    /// A bare `UserPromptSubmit` marks the turn in flight, so tests that go on to
    /// dispatch a branch/quoted send must start from an idle session — otherwise
    /// that send would be deferred behind the still-open registration turn.
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
}
