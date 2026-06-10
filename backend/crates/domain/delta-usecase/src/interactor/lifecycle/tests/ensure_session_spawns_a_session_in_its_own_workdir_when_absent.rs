use crate::interactor::testing::*;
use crate::ports::SessionLifecycle;

#[tokio::test]
async fn ensure_session_spawns_a_session_in_its_own_workdir_when_absent() {
    let ix = interactor();

    let status = ix.ensure_session().await.unwrap();

    // A fresh cold start reports `Starting` and spawns a session in its own
    // per-token workdir under the base, with the settings written to Delta's own
    // path (not the workdir) and passed via `--settings`.
    assert_eq!(status, SessionLifecycle::Starting);
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session was spawned");
    assert_eq!(created[0].name, "delta-1", "named after the minted token");
    assert_eq!(created[0].workdir, "/work/delta-1", "<base>/<token>");
    // The launched argv pins the conversation's session id with the id Delta
    // minted and recorded on the pending spawn.
    let minted = ix.pending_session_ids().await.remove(0);
    assert_eq!(
        created[0].command,
        vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            TEST_SETTINGS_PATH.to_owned(),
            "--session-id".to_owned(),
            minted.as_str().to_owned(),
        ],
        "claude --settings <delta path> --session-id <minted id>"
    );
    let written = ix.workspace_fake().written.lock().unwrap().clone();
    assert_eq!(
        written,
        vec![(TEST_SETTINGS_PATH.to_owned(), TEST_SETTINGS_JSON.to_owned())],
        "settings go to Delta's path, not the spawn workdir"
    );
}
