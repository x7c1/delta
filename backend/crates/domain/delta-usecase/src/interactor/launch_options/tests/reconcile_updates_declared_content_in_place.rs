use delta_model::LaunchOptionPreset;

use crate::interactor::testing::*;

use super::preset;

/// Reconciliation rewrites a shipped row's `label`, `name` and `value` when the
/// declared catalog changes them, in place.
#[tokio::test]
async fn reconcile_updates_declared_content_in_place() {
    let ix = interactor();
    ix.reconcile_builtin_launch_options(&[preset("claude:model-opus", "Opus", "opus")])
        .await
        .unwrap();
    let before = ix.list_launch_options().await.unwrap().remove(0);

    ix.reconcile_builtin_launch_options(&[LaunchOptionPreset {
        key: "claude:model-opus",
        label: "Opus (latest)",
        name: "--model-alias",
        value: Some("opus-latest"),
        provider: crate::AgentProvider::Claude,
    }])
    .await
    .unwrap();

    let after = ix.list_launch_options().await.unwrap();
    assert_eq!(after.len(), 1, "the row was updated, not replaced");
    assert_eq!(after[0].id, before.id);
    assert_eq!(after[0].label.as_deref(), Some("Opus (latest)"));
    assert_eq!(after[0].name, "--model-alias");
    assert_eq!(after[0].value.as_deref(), Some("opus-latest"));
}
