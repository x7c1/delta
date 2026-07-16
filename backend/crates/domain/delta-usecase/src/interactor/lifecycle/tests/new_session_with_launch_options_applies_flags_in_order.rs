use crate::interactor::testing::*;
use crate::SendTarget;

/// A composer-first send selecting registered launch options resolves each id
/// to its `(name, value?)` flag and pushes them onto the launch argv — in the
/// user's selection order, after Delta's own `--settings`/`--session-id` flags
/// and before the trailing positional prompt. A valueless option contributes
/// only its name, and a selected id that is no longer registered is skipped
/// rather than aborting the launch.
#[tokio::test]
async fn new_session_with_launch_options_applies_flags_in_order() {
    let ix = interactor();

    // Register three options; the picker would normally surface these.
    let permission_mode = ix
        .store()
        .create_launch_option(None, "--permission-mode", Some("auto"), false)
        .await
        .unwrap();
    let _plugin_dir = ix
        .store()
        .create_launch_option(None, "--plugin-dir", Some("/plugins"), false)
        .await
        .unwrap();
    let verbose = ix
        .store()
        .create_launch_option(None, "--verbose", None, false)
        .await
        .unwrap();

    // Select the valueless `--verbose` first, then `--permission-mode auto`,
    // and finally an id that was never registered. The non-id order proves the
    // argv follows selection order (not id/registry order), `--plugin-dir` is
    // left out, and the unknown id is skipped.
    ix.enqueue_send(
        SendTarget::NewSession {
            provider: crate::AgentProvider::Claude,
            workdir: None,
            launch_option_ids: vec![verbose.id, permission_mode.id, 9999],
            worktree: None,
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session spawned");
    let minted = ix.pending_session_ids().await.remove(0);
    assert_eq!(
        created[0].command,
        vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            TEST_SETTINGS_PATH.to_owned(),
            "--session-id".to_owned(),
            minted.as_str().to_owned(),
            // Selected options, in selection order; `--verbose` is valueless,
            // the unknown id 9999 is skipped.
            "--verbose".to_owned(),
            "--permission-mode".to_owned(),
            "auto".to_owned(),
            // The first prompt stays the trailing positional argument.
            "hello".to_owned(),
        ],
        "launch-option flags sit between --session-id and the positional prompt, in selection order"
    );
}
