use delta_model::{AgentProvider, SessionStatus};

use crate::interactor::testing::*;
use crate::SendTarget;

/// A composer send that came from the new-session screen's PR tab records the
/// PR's number on the **eager** `spawning` row, before the launch has bound.
/// That is what lets the navigator card show `#<number>` from the moment the
/// row is listed rather than only once the session registers.
#[tokio::test]
async fn new_session_from_a_pr_records_the_number_on_the_spawning_row() {
    let ix = interactor();

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: Some(138),
                provider: AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "resume PR work",
            None,
        )
        .await
        .unwrap();

    let session = ix
        .store()
        .session(&send.session_id)
        .await
        .unwrap()
        .expect("the eager spawning row was written");
    assert_eq!(
        session.status,
        SessionStatus::Spawning,
        "the number is on the row while it is still starting",
    );
    assert_eq!(session.pull_request_number, Some(138));
}

/// A session started from the Repository/Directory tab carries no PR origin:
/// the column stays NULL and the card renders nothing in the slot.
#[tokio::test]
async fn new_session_without_a_pr_records_no_number() {
    let ix = interactor();

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .unwrap();

    let session = ix.store().session(&send.session_id).await.unwrap().unwrap();
    assert_eq!(session.pull_request_number, None);
}

/// The Codex spawn path records the same snapshot: a PR-origin start is the
/// path a "start a session from a PR" click takes on either provider, so the
/// terminal-less path must not drop it.
#[tokio::test]
async fn a_codex_session_from_a_pr_records_the_number_too() {
    let factory = FakeAgentFactory::new("thr_fake", Some("turn_fake"));
    let ix = interactor_with_codex_factory(factory);

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: Some(174),
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "resume PR work on codex",
            None,
        )
        .await
        .unwrap();
    ix.await_launch().await;

    let session = ix.store().session(&send.session_id).await.unwrap().unwrap();
    assert_eq!(session.provider, AgentProvider::Codex);
    assert_eq!(session.pull_request_number, Some(174));
}

/// The number is a spawn-time snapshot, so neither the first hook's activation
/// nor a later close-and-resume touches it. Resuming a PR session must still
/// show the PR it belongs to.
#[tokio::test]
async fn the_pull_request_number_survives_close_and_resume() {
    let ix = interactor();

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: Some(138),
                provider: AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "resume PR work",
            None,
        )
        .await
        .unwrap();
    let id = send.session_id.clone();
    ix.await_launch().await;

    // The launch's first hook activates the row (spawning → active). That path
    // rewrites `cwd`/`transcript_path` and must leave the snapshot alone.
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "resume PR work",
    ))
    .await
    .unwrap();
    let activated = ix.store().session(&id).await.unwrap().unwrap();
    assert_eq!(activated.status, SessionStatus::Active);
    assert_eq!(
        activated.pull_request_number,
        Some(138),
        "registration must not clear the spawn-time snapshot",
    );

    ix.close_session(&id).await.unwrap();
    assert_eq!(
        ix.store()
            .session(&id)
            .await
            .unwrap()
            .unwrap()
            .pull_request_number,
        Some(138),
        "closing a session keeps its PR origin",
    );

    ix.open_session(&id).await.unwrap();
    assert_eq!(
        ix.store()
            .session(&id)
            .await
            .unwrap()
            .unwrap()
            .pull_request_number,
        Some(138),
        "and a resume re-reads the same stored row rather than rewriting it",
    );
}
