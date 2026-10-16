use std::sync::Mutex;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::Interactor;

/// A resume names its tmux session the way a fresh spawn does: it skips names
/// a surviving `delta-<n>` session from a previous server run still holds, so
/// `claude --resume` never fails with tmux's "duplicate session" after a
/// restart that left old panes alive.
#[tokio::test]
async fn resume_skips_tmux_session_names_already_in_use() {
    let ix = Interactor::new(
        FakeTmux {
            // A pane from a previous run survived the restart.
            live: Mutex::new(vec!["delta-1".to_owned()]),
            ..Default::default()
        },
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        FakeGitWorktree::default(),
        TEST_WORKTREE_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    );
    // A known-but-closed session to resume (an external claude in /elsewhere).
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();

    ix.open_session(&SessionId::from("sess-R")).await.unwrap();

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "exactly one session was created");
    assert!(
        created[0].command.iter().any(|a| a == "--resume"),
        "the created session is the resume: {:?}",
        created[0].command
    );
    assert_eq!(
        created[0].name, "delta-2",
        "the resume skipped the surviving name and minted the next free one",
    );
}
