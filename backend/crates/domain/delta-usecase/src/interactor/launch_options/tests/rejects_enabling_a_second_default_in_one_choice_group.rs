use super::interactor_with_a_vocabulary;

/// Turning a row's default on is refused while a sibling holds the group's
/// default; clearing the sibling first — the two writes a client makes to
/// switch the default — then lets it through. Clearing is always allowed, and
/// re-enabling the holder itself is not a conflict with itself.
#[tokio::test]
async fn rejects_enabling_a_second_default_in_one_choice_group() {
    let ix = interactor_with_a_vocabulary();
    let fable = ix
        .create_launch_option(
            Some("Fable"),
            "--model",
            Some("fable"),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    let custom = ix
        .create_launch_option(
            None,
            "--model",
            Some("e"),
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();

    let err = ix
        .set_launch_option_default_enabled(custom.id, true)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, crate::Error::LaunchOptionRejected(message)
            if message.contains("Fable")),
        "the refusal names the row holding the default, got {err:?}"
    );

    // The holder may be re-enabled: it is not a sibling of itself.
    ix.set_launch_option_default_enabled(fable.id, true)
        .await
        .unwrap();

    // Clear, then set.
    let cleared = ix
        .set_launch_option_default_enabled(fable.id, false)
        .await
        .unwrap()
        .unwrap();
    assert!(!cleared.default_enabled);
    let switched = ix
        .set_launch_option_default_enabled(custom.id, true)
        .await
        .unwrap()
        .unwrap();
    assert!(switched.default_enabled);
}
