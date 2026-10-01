use crate::interactor::testing::*;

/// The startup refresh writes the configured settings JSON to the configured
/// path, without any session being spawned or resumed.
#[tokio::test]
async fn refresh_session_settings_writes_the_configured_file() {
    let ix = interactor();

    ix.refresh_session_settings().await.unwrap();

    let written = ix.workspace_fake().written.lock().unwrap().clone();
    assert_eq!(
        written,
        vec![(
            ix.session_settings_path.clone(),
            ix.session_settings_json.clone()
        )]
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "refreshing the settings launches nothing"
    );
}
