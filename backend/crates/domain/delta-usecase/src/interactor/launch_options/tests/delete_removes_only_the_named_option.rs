use crate::interactor::testing::*;

/// Delete removes only the named option; deleting an unknown id is a no-op.
#[tokio::test]
async fn delete_removes_only_the_named_option() {
    let ix = interactor();
    let keep = ix
        .create_launch_option(
            None,
            "--model",
            Some("opus"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let drop = ix
        .create_launch_option(
            None,
            "--model",
            Some("sonnet"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();

    ix.delete_launch_option(drop.id).await.unwrap();
    let remaining = ix.list_launch_options().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, keep.id);

    // Deleting an id that no longer exists is silently fine.
    ix.delete_launch_option(drop.id).await.unwrap();
    assert_eq!(ix.list_launch_options().await.unwrap().len(), 1);
}
