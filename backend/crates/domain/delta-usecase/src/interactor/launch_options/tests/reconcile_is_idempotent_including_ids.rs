use crate::interactor::testing::*;

use super::preset;

/// Reconciliation is idempotent, ids included.
///
/// Ids matter more than row counts here: a shipped row's id can already sit in a
/// saved composer selection, so a reconcile that recreated rows would quietly
/// invalidate the user's selection on every restart.
#[tokio::test]
async fn reconcile_is_idempotent_including_ids() {
    let ix = interactor();
    let catalog = [
        preset("claude:model-opus", "Opus", "opus"),
        preset("claude:model-sonnet", "Sonnet", "sonnet"),
    ];

    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();
    let first = ix.list_launch_options().await.unwrap();
    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();
    let second = ix.list_launch_options().await.unwrap();

    assert_eq!(first, second);
}
