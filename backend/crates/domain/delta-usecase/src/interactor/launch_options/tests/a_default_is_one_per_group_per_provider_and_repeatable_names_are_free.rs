use super::interactor_with_a_vocabulary;

/// The group is per provider: the same single-valued name under another
/// provider is a different group with its own default, and a repeatable name
/// may carry any number of defaults.
#[tokio::test]
async fn a_default_is_one_per_group_per_provider_and_repeatable_names_are_free() {
    let ix = interactor_with_a_vocabulary();
    for provider in [crate::AgentProvider::Claude, crate::AgentProvider::Codex] {
        ix.create_launch_option(None, "--model", Some("a"), true, provider)
            .await
            .unwrap();
    }
    for value in ["/a", "/b"] {
        ix.create_launch_option(
            None,
            "--plugin-dir",
            Some(value),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    }
    assert_eq!(ix.list_launch_options().await.unwrap().len(), 4);
}
