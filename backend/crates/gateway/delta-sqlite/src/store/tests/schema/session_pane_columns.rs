//! The forward migrations of the session's pane-related columns: the v10
//! remembered pane and the v11 close reason, which a binding clears.

use delta_model::SessionId;

use super::super::super::SqliteStore;
use super::super::new_session;
use super::{downgrade_sql, read_user_version};

/// A database written by the generation before the remembered pane — stamped 9
/// — is migrated forward on open: the columns arrive, the file is re-stamped,
/// and the session that was already there remembers no pane, so a restart
/// re-adopts nothing from it (no earlier binary recorded where a pane was).
#[tokio::test]
async fn a_v9_database_gains_the_remembered_pane_and_keeps_its_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v9.sqlite");
    let path_str = path.to_str().unwrap();

    {
        let store = SqliteStore::open(path_str).unwrap();
        store.register_session(new_session()).await.unwrap();
        let conn = store.conn.lock().await;
        conn.execute_batch(&downgrade_sql(9)).unwrap();
    }
    assert_eq!(read_user_version(path_str), 9);

    let store = SqliteStore::open(path_str).unwrap();
    assert_eq!(read_user_version(path_str), crate::SCHEMA_VERSION);

    let id = SessionId::from("sess-1");
    assert!(
        store.session(&id).await.unwrap().is_some(),
        "the pre-existing session survives the migration"
    );
    assert_eq!(store.remembered_pane(&id).await.unwrap(), None);
    assert!(store.remembered_panes().await.unwrap().is_empty());

    // The new columns are usable through the normal write path.
    let pane = delta_usecase::RememberedPane {
        tmux_session: "delta-1".into(),
        pane: "delta-1:0.0".into(),
        hooks_unreachable: true,
    };
    store.remember_pane(&id, &pane).await.unwrap();
    assert_eq!(store.remembered_pane(&id).await.unwrap(), Some(pane));
}

#[tokio::test]
async fn a_v10_database_gains_closed_reason_and_keeps_its_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v10.sqlite");
    let path_str = path.to_str().unwrap();

    {
        let store = SqliteStore::open(path_str).unwrap();
        store.register_session(new_session()).await.unwrap();
        let conn = store.conn.lock().await;
        conn.execute_batch(&downgrade_sql(10)).unwrap();
    }
    assert_eq!(read_user_version(path_str), 10);

    let store = SqliteStore::open(path_str).unwrap();
    assert_eq!(read_user_version(path_str), crate::SCHEMA_VERSION);

    let id = SessionId::from("sess-1");
    let session = store
        .session(&id)
        .await
        .unwrap()
        .expect("the pre-existing session survives the migration");
    assert_eq!(session.closed_reason, None);

    // The new column is usable through the normal write path, and binding a
    // pane clears it.
    store
        .record_closed_reason(&id, "moved to a new port")
        .await
        .unwrap();
    assert_eq!(
        store
            .session(&id)
            .await
            .unwrap()
            .unwrap()
            .closed_reason
            .as_deref(),
        Some("moved to a new port")
    );
    let pane = delta_usecase::RememberedPane {
        tmux_session: "delta-1".into(),
        pane: "delta-1:0.0".into(),
        hooks_unreachable: false,
    };
    store.remember_pane(&id, &pane).await.unwrap();
    assert_eq!(
        store.session(&id).await.unwrap().unwrap().closed_reason,
        None
    );
}
