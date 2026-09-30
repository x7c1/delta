use crate::interactor::testing::*;

use super::preset;

/// Reconciliation materializes a declared preset that is not in the registry,
/// with `default_enabled` off: a shipped option is offered, never imposed.
#[tokio::test]
async fn reconcile_inserts_a_declared_preset_as_a_real_row() {
    let ix = interactor();
    let catalog = [preset("claude:model-opus", "Opus", "opus")];

    ix.reconcile_builtin_launch_options(&catalog).await.unwrap();

    let listed = ix.list_launch_options().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].builtin_key.as_deref(), Some("claude:model-opus"));
    assert_eq!(listed[0].label.as_deref(), Some("Opus"));
    assert_eq!(listed[0].name, "--model");
    assert_eq!(listed[0].value.as_deref(), Some("opus"));
    assert!(!listed[0].default_enabled);
}
