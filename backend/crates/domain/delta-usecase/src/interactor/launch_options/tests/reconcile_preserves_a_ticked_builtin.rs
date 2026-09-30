use crate::interactor::testing::*;

use super::preset;

/// Reconciliation preserves `default_enabled` — and only `default_enabled`.
///
/// This is the property the whole design rests on: it is what makes it safe for
/// startup to overwrite a shipped row's `label`/`name`/`value` from the catalog
/// every single time, because those three cannot be edited through the API
/// anyway while this one can.
#[tokio::test]
async fn reconcile_preserves_a_ticked_builtin() {
    let ix = interactor();
    let catalog = [preset("claude:model-opus", "Opus", "opus")];
    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();
    let shipped = ix.list_launch_options().await.unwrap().remove(0);

    ix.set_launch_option_default_enabled(shipped.id, true)
        .await
        .unwrap()
        .expect("the shipped row exists");

    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();

    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, shipped.id);
    assert!(
        listed[0].default_enabled,
        "the user's tick survives a reconcile"
    );
}
