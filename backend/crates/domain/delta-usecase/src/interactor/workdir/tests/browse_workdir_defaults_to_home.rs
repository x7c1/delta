use crate::interactor::testing::*;

/// `browse_workdir` defaults to `$HOME` when no path is given and delegates to
/// the workspace port's listing.
#[tokio::test]
async fn browse_workdir_defaults_to_home() {
    let ix = interactor();
    // Pin HOME for a deterministic default.
    std::env::set_var("HOME", "/home/tester");

    let listing = ix.browse_workdir(None).await.unwrap();
    assert_eq!(
        listing.path,
        FakeWorkspace::canonical("/home/tester"),
        "an absent path browses $HOME"
    );

    // An explicit path is used verbatim (then canonicalized by the port).
    let explicit = ix.browse_workdir(Some("/srv")).await.unwrap();
    assert_eq!(explicit.path, FakeWorkspace::canonical("/srv"));
}
