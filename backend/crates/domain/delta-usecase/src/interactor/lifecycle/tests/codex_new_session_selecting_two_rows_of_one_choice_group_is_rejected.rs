use std::sync::Arc;

use delta_model::AgentProvider;

use crate::error::Error;
use crate::interactor::testing::*;
use crate::SendTarget;

/// The adapter-backed spawn enforces the same rule as the pane spawn: two
/// selected rows of one single-valued name are refused with
/// [`Error::LaunchOptionRejected`] in the domain, before the eager row is
/// written and before the adapter is asked for anything — so the rule does not
/// depend on each adapter re-checking it.
///
/// Two rows of a repeatable name still reach the adapter together.
#[tokio::test]
async fn codex_new_session_selecting_two_rows_of_one_choice_group_is_rejected() {
    let factory = FakeAgentFactory::new("thr_group", Some("turn_group"));
    let ix = interactor_with_codex_factory(factory.clone())
        .with_launch_option_vocabulary(Arc::new(FakeLaunchOptionVocabulary));

    let mut models = Vec::new();
    for (label, value) in [(Some("Sol"), "gpt-5.6-sol"), (Some("Luna"), "gpt-5.6-luna")] {
        models.push(
            ix.store()
                .create_launch_option(label, "model", Some(value), false, AgentProvider::Codex)
                .await
                .unwrap()
                .id,
        );
    }

    let err = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: models,
                worktree: None,
            },
            "hello codex",
            None,
        )
        .await
        .expect_err("two values of a single-valued option must fail the send");
    assert!(
        matches!(&err, Error::LaunchOptionRejected(message)
            if message.contains("model") && message.contains("Sol") && message.contains("Luna")),
        "the refusal names the option and both selected rows, got {err:?}"
    );

    ix.await_launch().await;
    assert!(
        ix.store().inner.lock().unwrap().sessions.is_empty(),
        "a refused selection leaves no session row"
    );
    assert!(
        ix.launching_session_ids().await.is_empty(),
        "no launch was recorded"
    );
    {
        let log = factory.log();
        let log = log.lock().unwrap();
        assert!(
            log.launches.is_empty(),
            "the adapter was never asked to launch"
        );
    }

    // Two rows of a repeatable name are independent and reach the adapter.
    let mut configs = Vec::new();
    for value in ["a=1", "b=2"] {
        configs.push(
            ix.store()
                .create_launch_option(None, "config", Some(value), false, AgentProvider::Codex)
                .await
                .unwrap()
                .id,
        );
    }
    ix.enqueue_send(
        SendTarget::NewSession {
            pull_request_number: None,
            provider: AgentProvider::Codex,
            workdir: None,
            launch_option_ids: configs,
            worktree: None,
        },
        "hello codex",
        None,
    )
    .await
    .expect("two repeatable rows launch together");
    ix.await_launch().await;
    let launches = {
        let log = factory.log();
        let log = log.lock().unwrap();
        log.launches.clone()
    };
    assert_eq!(launches.len(), 1, "one launch");
    assert_eq!(launches[0].launch_options.len(), 2);
}
