use crate::interactor::testing::*;

use super::interactor_with_a_vocabulary;

/// `default_enabled` cannot be turned on for a dangerous row, but turning it
/// off always works, which is how a row that predates the rule is disarmed.
#[tokio::test]
async fn set_default_enabled_refuses_a_dangerous_option() {
    let ix = interactor_with_a_vocabulary();
    // Undefaulted, which is the only shape the create path lets a dangerous
    // option in as.
    let dangerous = ix
        .create_launch_option(
            None,
            DANGEROUS_NAME,
            None,
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();

    let err = ix
        .set_launch_option_default_enabled(dangerous.id, true)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, crate::Error::LaunchOptionRejected(message)
            if message.contains(DANGEROUS_NAME)),
        "expected a rejection naming the option, got {err:?}"
    );

    // Disabling is allowed, and an id nobody has is still a `None` (a 404 at the
    // REST layer) rather than a rejection.
    assert!(
        !ix.set_launch_option_default_enabled(dangerous.id, false)
            .await
            .unwrap()
            .expect("the row exists")
            .default_enabled
    );
    assert!(ix
        .set_launch_option_default_enabled(9999, true)
        .await
        .unwrap()
        .is_none());
}
