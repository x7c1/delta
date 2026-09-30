use crate::interactor::testing::*;

/// Without an injected vocabulary nothing is grouped and nothing is refused:
/// every row is its own option, and two defaults of one name are accepted as
/// they were before the rule existed.
#[tokio::test]
async fn the_null_vocabulary_groups_and_rejects_nothing() {
    let ix = interactor();
    let first = ix
        .create_launch_option(
            None,
            "--model",
            Some("a"),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let second = ix
        .create_launch_option(
            None,
            "--model",
            Some("b"),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert_eq!(ix.launch_option_choice_group(&first), None);
    assert_eq!(ix.launch_option_choice_group(&second), None);
}
