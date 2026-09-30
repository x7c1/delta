use crate::interactor::testing::*;

/// Setting `default_enabled` toggles it in place and returns the updated row;
/// an unknown id returns `None`.
#[tokio::test]
async fn set_default_enabled_toggles_in_place() {
    let ix = interactor();
    let option = ix
        .create_launch_option(
            None,
            "--plugin-dir",
            Some("/opt/p"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert!(!option.default_enabled);

    let updated = ix
        .set_launch_option_default_enabled(option.id, true)
        .await
        .unwrap()
        .expect("an existing option");
    assert_eq!(updated.id, option.id);
    assert!(updated.default_enabled);

    let listed = ix.list_launch_options().await.unwrap();
    assert!(
        listed
            .iter()
            .find(|o| o.id == option.id)
            .unwrap()
            .default_enabled
    );

    assert!(ix
        .set_launch_option_default_enabled(9999, true)
        .await
        .unwrap()
        .is_none());
}
