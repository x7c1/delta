use crate::interactor::testing::*;

/// Create then list returns the registered option with every field preserved,
/// and the list is newest-first.
#[tokio::test]
async fn create_then_list_returns_registered_options_newest_first() {
    let ix = interactor();

    let first = ix
        .create_launch_option(
            Some("plugins"),
            "--plugin-dir",
            Some("/opt/p"),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let second = ix
        .create_launch_option(
            None,
            "--permission-mode",
            Some("auto"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();

    let listed = ix.list_launch_options().await.unwrap();
    let ids: Vec<i64> = listed.iter().map(|o| o.id).collect();
    assert_eq!(ids, vec![second.id, first.id], "newest first");

    let plugins = listed.iter().find(|o| o.id == first.id).unwrap();
    assert_eq!(plugins.label.as_deref(), Some("plugins"));
    assert_eq!(plugins.name, "--plugin-dir");
    assert_eq!(plugins.value.as_deref(), Some("/opt/p"));
    assert!(plugins.default_enabled);
    assert_eq!(plugins.provider, crate::AgentProvider::Claude);
}
