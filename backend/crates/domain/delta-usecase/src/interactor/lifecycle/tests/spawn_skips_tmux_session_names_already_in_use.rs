use std::sync::Mutex;

use crate::interactor::testing::*;
use crate::Interactor;

/// A spawn skips tmux session names that already exist (surviving `delta-<n>`
/// sessions from a previous server run), so it never fails with tmux's
/// "duplicate session". The minter resets to `delta-1` on each start, so without
/// this a restart that left old panes alive would re-mint a colliding name.
#[tokio::test]
async fn spawn_skips_tmux_session_names_already_in_use() {
    let ix = Interactor::new(
        FakeTmux {
            // Two panes from a previous run survived the restart.
            live: Mutex::new(vec!["delta-1".to_owned(), "delta-2".to_owned()]),
            ..Default::default()
        },
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        FakeGitWorktree::default(),
        TEST_WORKDIR_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    );

    let token = ix.new_session().await.expect("spawn does not collide");

    assert_eq!(
        token.as_str(),
        "delta-3",
        "the spawn skipped the two surviving names and minted the next free one",
    );
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "exactly one session was created");
    assert_eq!(created[0].name, "delta-3", "created under the free name");
    assert_eq!(created[0].workdir, "/work/delta-3", "<base>/<free token>");
}
