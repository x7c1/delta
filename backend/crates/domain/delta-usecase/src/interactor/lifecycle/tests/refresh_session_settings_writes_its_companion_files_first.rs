use crate::interactor::testing::*;
use crate::SessionSettingsFile;

/// The files wired to go next to the settings file are written with it, ahead
/// of it: the settings' command entries read them, so the settings file never
/// lands on disk pointing at a file that is not there yet.
#[tokio::test]
async fn refresh_session_settings_writes_its_companion_files_first() {
    let companion = SessionSettingsFile {
        path: "/data/settings/4000-status-line.curl".to_owned(),
        contents: "url = \"http://127.0.0.1:4000/hooks/status-line\"\n".to_owned(),
    };
    let ix = interactor().with_session_settings_files(vec![companion.clone()]);

    ix.refresh_session_settings().await.unwrap();

    let written = ix.workspace_fake().written.lock().unwrap().clone();
    assert_eq!(
        written,
        vec![
            (companion.path, companion.contents),
            (
                ix.session_settings_path.clone(),
                ix.session_settings_json.clone()
            ),
        ]
    );
}
