use super::interactor_with_a_vocabulary;

/// A choice group holds one default: creating a second default-enabled row of
/// a single-valued name is refused, naming the row that holds the default, and
/// stores nothing. The same row undefaulted is created normally.
#[tokio::test]
async fn rejects_creating_a_second_default_in_one_choice_group() {
    let ix = interactor_with_a_vocabulary();
    ix.create_launch_option(
        Some("Fable"),
        "--model",
        Some("fable"),
        true,
        crate::AgentProvider::Claude,
    )
    .await
    .unwrap();

    let err = ix
        .create_launch_option(
            None,
            "--model",
            Some("e"),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&err, crate::Error::LaunchOptionRejected(message)
            if message.contains("--model") && message.contains("Fable")),
        "the refusal names the group and the row holding its default, got {err:?}"
    );
    assert_eq!(
        ix.list_launch_options().await.unwrap().len(),
        1,
        "a refused create stores nothing"
    );

    let undefaulted = ix
        .create_launch_option(
            None,
            "--model",
            Some("e"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert!(!undefaulted.default_enabled);
}
