use crate::interactor::testing::*;

use super::preset;

/// A shipped row is not the user's to delete: the refusal is
/// [`Error::LaunchOptionIsBuiltin`] and the row survives. Their own rows, and an
/// id nobody has, are unaffected.
#[tokio::test]
async fn delete_refuses_a_shipped_option() {
    let ix = interactor();
    ix.reconcile_builtin_launch_options(&[preset("claude:model-opus", "Opus", "opus")])
        .await
        .unwrap();
    let shipped = ix.list_launch_options().await.unwrap().remove(0);

    let err = ix.delete_launch_option(shipped.id).await.unwrap_err();
    assert!(
        matches!(err, crate::Error::LaunchOptionIsBuiltin(id) if id == shipped.id),
        "expected a built-in refusal naming the id, got {err:?}"
    );
    assert_eq!(ix.list_launch_options().await.unwrap().len(), 1);

    // An unknown id is still a silent no-op, not a refusal.
    ix.delete_launch_option(9999).await.unwrap();
}
