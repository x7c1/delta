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

// Helper accessors used only in tests to reach into the fakes the interactor owns.
impl Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace> {
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
