use crate::interactor::testing::*;

use super::interactor_with_a_vocabulary;

/// A dangerous option can be registered, but never as default-enabled: the
/// create is refused with [`Error::LaunchOptionRejected`] and nothing is stored,
/// while the same option undefaulted is created normally.
///
/// [`Error::LaunchOptionRejected`]: crate::Error::LaunchOptionRejected
#[tokio::test]
async fn create_refuses_a_dangerous_option_as_default_enabled() {
    let ix = interactor_with_a_vocabulary();

    let err = ix
        .create_launch_option(
            None,
            DANGEROUS_NAME,
            None,
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&err, crate::Error::LaunchOptionRejected(message)
            if message.contains(DANGEROUS_NAME)),
        "the refusal must name the offending option, got {err:?}"
    );
    assert!(
        ix.list_launch_options().await.unwrap().is_empty(),
        "a refused create stores nothing"
    );

    // Undefaulted it is an ordinary row, and still marked as dangerous.
    let created = ix
        .create_launch_option(
            None,
            DANGEROUS_NAME,
            None,
            false,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert!(!created.default_enabled);
    assert!(ix.is_launch_option_dangerous(&created));

    // A benign option may still be default-enabled.
    let benign = ix
        .create_launch_option(
            None,
            "--model",
            Some("opus"),
            true,
            crate::AgentProvider::Claude,
        )
        .await
        .unwrap();
    assert!(benign.default_enabled);
    assert!(!ix.is_launch_option_dangerous(&benign));
}
