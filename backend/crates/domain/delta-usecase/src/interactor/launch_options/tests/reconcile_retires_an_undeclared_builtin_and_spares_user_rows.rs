use crate::interactor::testing::*;

use super::preset;

/// A preset dropped from the catalog is retired from the registry, and the
/// user's own rows are untouched by the sweep.
#[tokio::test]
async fn reconcile_retires_an_undeclared_builtin_and_spares_user_rows() {
    let ix = interactor();
    let mine = ix
        .create_launch_option(
            Some("mine"),
            "--plugin-dir",
            Some("/opt/p"),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    ix.reconcile_builtin_launch_options(&[
        preset("claude:model-opus", "Opus", "opus"),
        preset("claude:model-sonnet", "Sonnet", "sonnet"),
    ])
    .await
    .unwrap();
    assert_eq!(ix.list_launch_options().await.unwrap().len(), 3);

    ix.reconcile_builtin_launch_options(&[preset("claude:model-opus", "Opus", "opus")])
        .await
        .unwrap();

    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].builtin_key.as_deref(), Some("claude:model-opus"));
    assert_eq!(listed[1].id, mine.id, "the user's own row is out of scope");
    assert!(listed[1].default_enabled, "and keeps its flag");

    // An empty catalog retires every shipped row and still spares the user's.
    ix.reconcile_builtin_launch_options(&[]).await.unwrap();
    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, mine.id);
}
