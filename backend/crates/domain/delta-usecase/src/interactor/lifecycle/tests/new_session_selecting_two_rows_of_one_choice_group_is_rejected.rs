use std::sync::Arc;

use crate::error::Error;
use crate::interactor::testing::*;
use crate::SendTarget;

/// A Claude send selecting two rows of one choice group — two `--model` rows,
/// a name the vocabulary classifies as single-valued — fails the send itself
/// with [`Error::LaunchOptionRejected`] naming both rows, before anything is
/// minted: no pane token, no tmux session, no pending spawn. Without the check
/// both rows would reach the launch argv and leave the choice to the CLI.
///
/// Two rows of a repeatable name (`--plugin-dir`) are independent options and
/// still launch together.
#[tokio::test]
async fn new_session_selecting_two_rows_of_one_choice_group_is_rejected() {
    let ix = interactor().with_launch_option_vocabulary(Arc::new(FakeLaunchOptionVocabulary));

    let fable = ix
        .store()
        .create_launch_option(
            Some("Fable"),
            "--model",
            Some("fable"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let custom = ix
        .store()
        .create_launch_option(
            None,
            "--model",
            Some("e"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();

    let err = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: None,
                launch_option_ids: vec![fable.id, custom.id],
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .expect_err("two values of a single-valued option must fail the send");
    assert!(
        matches!(&err, Error::LaunchOptionRejected(message)
            if message.contains("--model")
                && message.contains("Fable")
                && message.contains("--model e")),
        "the refusal names the option and both selected rows, got {err:?}"
    );

    ix.await_launch().await;
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "nothing was launched"
    );
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "no pane token was minted"
    );
    assert!(
        ix.store().inner.lock().unwrap().sessions.is_empty(),
        "no session row was written"
    );

    // Two rows of a repeatable name are independent and launch together.
    let mut plugin_dirs = Vec::new();
    for dir in ["/a", "/b"] {
        plugin_dirs.push(
            ix.store()
                .create_launch_option(
                    None,
                    "--plugin-dir",
                    Some(dir),
                    false,
                    crate::AgentProvider::Claude,
                )
                .await
                .unwrap()
                .id,
        );
    }
    ix.enqueue_send(
        SendTarget::NewSession {
            pull_request_number: None,
            provider: crate::AgentProvider::Claude,
            workdir: None,
            launch_option_ids: plugin_dirs,
            worktree: None,
        },
        "hello",
        None,
    )
    .await
    .expect("two repeatable rows launch together");
    ix.await_launch().await;
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session spawned");
    let command = &created[0].command;
    assert_eq!(
        command.iter().filter(|arg| *arg == "--plugin-dir").count(),
        2,
        "both repeatable rows reach the argv, got {command:?}"
    );
}
