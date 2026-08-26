use crate::interactor::testing::*;
use crate::SendTarget;

/// A selected launch option whose value starts with `~/` has the tilde expanded
/// to `$HOME` before the value reaches the launch argv. The spawn command line
/// is forwarded to `claude` as an argv tail with no shell, so without this
/// expansion the literal `~/...` would reach `claude` and be resolved against
/// the (worktree) cwd into a bogus `<cwd>/~/...` path. The expanded value sits
/// right after its `--plugin-dir` name token.
#[tokio::test]
async fn new_session_with_launch_option_value_expands_leading_tilde() {
    // Tilde expansion keys off HOME; in a degenerate env without it the
    // expansion is a no-op, so there is nothing meaningful to assert.
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    if home.is_empty() {
        return;
    }

    let ix = interactor();

    let plugin_dir = ix
        .store()
        .create_launch_option(
            None,
            "--plugin-dir",
            Some("~/repos/x/plugins"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();

    ix.enqueue_send(
        SendTarget::NewSession {
            provider: crate::AgentProvider::Claude,
            workdir: None,
            launch_option_ids: vec![plugin_dir.id],
            worktree: None,
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    ix.await_launch().await;

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session spawned");
    let minted = ix.pending_session_ids().await.remove(0);
    let expanded = format!("{}/repos/x/plugins", home.trim_end_matches('/'));
    assert_eq!(
        created[0].command,
        vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            TEST_SETTINGS_PATH.to_owned(),
            "--session-id".to_owned(),
            minted.as_str().to_owned(),
            // The tilde value is expanded to $HOME, not forwarded literally,
            // and sits right after its `--plugin-dir` name token.
            "--plugin-dir".to_owned(),
            expanded,
            // The first prompt stays the trailing positional argument.
            "hello".to_owned(),
        ],
        "the launch-option value has its leading tilde expanded to $HOME"
    );
}
