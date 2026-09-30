use crate::interactor::testing::*;

/// The registry holds options for every provider (the list is not filtered
/// server-side); each option round-trips with its own provider preserved.
#[tokio::test]
async fn create_preserves_each_options_provider() {
    let ix = interactor();

    let claude = ix
        .create_launch_option(
            None,
            "--permission-mode",
            Some("auto"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let codex = ix
        .create_launch_option(
            None,
            "model",
            Some("gpt-5"),
            false,
            crate::AgentProvider::Codex,
        )
        .await
        .unwrap();

    assert_eq!(claude.provider, crate::AgentProvider::Claude);
    assert_eq!(codex.provider, crate::AgentProvider::Codex);

    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 2, "the list carries both providers' options");
    assert_eq!(
        listed.iter().find(|o| o.id == codex.id).unwrap().provider,
        crate::AgentProvider::Codex,
    );
}
