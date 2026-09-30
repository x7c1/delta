use crate::interactor::testing::*;

/// A valueless, unlabeled flag round-trips with `None` for both optionals.
#[tokio::test]
async fn create_valueless_flag_keeps_label_and_value_none() {
    let ix = interactor();
    let option = ix
        .create_launch_option(
            None,
            "--dangerously-skip-permissions",
            None,
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert_eq!(option.label, None);
    assert_eq!(option.value, None);
    assert_eq!(option.name, "--dangerously-skip-permissions");
    assert!(!option.default_enabled);
}
