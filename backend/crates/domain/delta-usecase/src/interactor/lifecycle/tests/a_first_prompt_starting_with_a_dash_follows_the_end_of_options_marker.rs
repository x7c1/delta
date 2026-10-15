use crate::interactor::testing::*;
use crate::SendTarget;

/// A first prompt that starts with `-` — a Markdown list item such as
/// `- fix the build` — rides the launch argv after a `--`, so `claude` reads
/// it as its positional prompt instead of rejecting it as an unknown option
/// and failing the spawn.
#[tokio::test]
async fn a_first_prompt_starting_with_a_dash_follows_the_end_of_options_marker() {
    let ix = interactor();

    ix.enqueue_send(
        SendTarget::NewSession {
            pull_request_number: None,
            provider: crate::AgentProvider::Claude,
            workdir: None,
            launch_option_ids: vec![],
            worktree: None,
        },
        "- fix the build",
        None,
    )
    .await
    .unwrap();

    ix.await_launch().await;

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session spawned");
    let command = &created[0].command;
    let tail: Vec<&str> = command[command.len() - 2..]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        ["--", "- fix the build"],
        "the dash-prefixed prompt follows the end-of-options marker: {command:?}"
    );
}
