//! Launch-option registry use-case tests.

use crate::interactor::testing::*;

/// Create then list returns the registered option with every field preserved,
/// and the list is newest-first.
#[tokio::test]
async fn create_then_list_returns_registered_options_newest_first() {
    let ix = interactor();

    let first = ix
        .create_launch_option(Some("plugins"), "--plugin-dir", Some("/opt/p"))
        .await
        .unwrap();
    let second = ix
        .create_launch_option(None, "--permission-mode", Some("auto"))
        .await
        .unwrap();

    let listed = ix.list_launch_options().await.unwrap();
    let ids: Vec<i64> = listed.iter().map(|o| o.id).collect();
    assert_eq!(ids, vec![second.id, first.id], "newest first");

    let plugins = listed.iter().find(|o| o.id == first.id).unwrap();
    assert_eq!(plugins.label.as_deref(), Some("plugins"));
    assert_eq!(plugins.name, "--plugin-dir");
    assert_eq!(plugins.value.as_deref(), Some("/opt/p"));
}

/// A valueless, unlabeled flag round-trips with `None` for both optionals.
#[tokio::test]
async fn create_valueless_flag_keeps_label_and_value_none() {
    let ix = interactor();
    let option = ix
        .create_launch_option(None, "--dangerously-skip-permissions", None)
        .await
        .unwrap();
    assert_eq!(option.label, None);
    assert_eq!(option.value, None);
    assert_eq!(option.name, "--dangerously-skip-permissions");
}

/// Delete removes only the named option; deleting an unknown id is a no-op.
#[tokio::test]
async fn delete_removes_only_the_named_option() {
    let ix = interactor();
    let keep = ix
        .create_launch_option(None, "--model", Some("opus"))
        .await
        .unwrap();
    let drop = ix
        .create_launch_option(None, "--model", Some("sonnet"))
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
