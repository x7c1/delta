use delta_model::AgentProvider;

use crate::agent::LaunchOptionSpec;
use crate::interactor::testing::*;
use crate::SendTarget;

/// A composer-first send that selects Codex-scoped launch options starts the
/// session (it used to be rejected outright) and hands the selection to the
/// adapter on the launch request, as neutral `(name, value?)` pairs in the
/// user's selection order.
///
/// The pairs reach the adapter un-rendered on purpose: turning `model` /
/// `gpt-5.6-sol` into a `thread/start` field is the Codex adapter's job, so the
/// core stays free of any provider's launch wire shape. A valueless option
/// keeps its `None`, and a selected id that is no longer registered is skipped
/// rather than aborting the launch — the same rules the Claude path follows.
#[tokio::test]
async fn new_session_with_codex_launch_options_reaches_the_adapter() {
    let factory = FakeAgentFactory::new("thr_options", Some("turn_options"));
    let ix = interactor_with_codex_factory(factory.clone());

    let model = ix
        .store()
        .create_launch_option(
            None,
            "model",
            Some("gpt-5.6-sol"),
            false,
            AgentProvider::Codex,
        )
        .await
        .unwrap();
    let sandbox = ix
        .store()
        .create_launch_option(
            None,
            "sandbox",
            Some("read-only"),
            false,
            AgentProvider::Codex,
        )
        .await
        .unwrap();
    let ephemeral = ix
        .store()
        .create_launch_option(None, "ephemeral", None, false, AgentProvider::Codex)
        .await
        .unwrap();

    // Select sandbox first, then the valueless `ephemeral`, then an id that was
    // never registered; `model` is left unselected. The non-id order proves the
    // launch request follows selection order, not registry order.
    ix.enqueue_send(
        SendTarget::NewSession {
            provider: AgentProvider::Codex,
            workdir: None,
            launch_option_ids: vec![sandbox.id, ephemeral.id, 9999],
            worktree: None,
        },
        "hello codex",
        None,
    )
    .await
    .expect("a Codex session with launch options starts");
    ix.await_launch().await;

    let launches = {
        let log = factory.log();
        let log = log.lock().unwrap();
        log.launches.clone()
    };
    assert_eq!(launches.len(), 1, "one launch (thread/start)");
    assert_eq!(
        launches[0].launch_options,
        vec![
            LaunchOptionSpec {
                name: "sandbox".to_owned(),
                value: Some("read-only".to_owned()),
            },
            LaunchOptionSpec {
                name: "ephemeral".to_owned(),
                value: None,
            },
        ],
        "the selected options reach the adapter in selection order, unrendered; \
         the unselected `model` ({}) and the unregistered id are absent",
        model.id
    );
}
