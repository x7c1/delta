use super::interactor_with_a_vocabulary;

/// With the vocabulary wired, a single-valued row's group is its name and a
/// repeatable row has none.
#[tokio::test]
async fn a_single_valued_row_is_grouped_by_its_name() {
    let ix = interactor_with_a_vocabulary();
    let model = ix
        .create_launch_option(
            None,
            "--model",
            Some("a"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let plugin = ix
        .create_launch_option(
            None,
            "--plugin-dir",
            Some("/p"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert_eq!(
        ix.launch_option_choice_group(&model).as_deref(),
        Some("--model")
    );
    assert_eq!(ix.launch_option_choice_group(&plugin), None);
}
