//! The session row's remembered pane: written when a pane is bound, read back
//! at boot, and cleared when the session closes.

use delta_model::SessionId;
use delta_usecase::RememberedPane;

use super::super::SqliteStore;
use super::new_session_with;

fn pane(n: u32) -> RememberedPane {
    RememberedPane {
        tmux_session: format!("delta-{n}"),
        pane: format!("delta-{n}:0.0"),
        hooks_unreachable: false,
    }
}

/// A freshly registered session remembers nothing; binding writes the pane,
/// a later bind (a re-adoption that marked the hooks unreachable) replaces it,
/// and closing clears it — the row reads "nothing remembered" again.
#[tokio::test]
async fn a_remembered_pane_is_written_on_bind_and_cleared_on_close() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store
        .register_session(new_session_with("sess-1"))
        .await
        .unwrap();
    assert_eq!(store.remembered_pane(&session.id).await.unwrap(), None);

    store.remember_pane(&session.id, &pane(3)).await.unwrap();
    assert_eq!(
        store.remembered_pane(&session.id).await.unwrap(),
        Some(pane(3))
    );

    let marked = RememberedPane {
        hooks_unreachable: true,
        ..pane(3)
    };
    store.remember_pane(&session.id, &marked).await.unwrap();
    assert_eq!(
        store.remembered_pane(&session.id).await.unwrap(),
        Some(marked)
    );

    store.forget_pane(&session.id).await.unwrap();
    assert_eq!(store.remembered_pane(&session.id).await.unwrap(), None);
    assert!(store.remembered_panes().await.unwrap().is_empty());
}

/// The boot listing names exactly the sessions that remember a pane, oldest
/// first, and skips the ones that do not.
#[tokio::test]
async fn remembered_panes_lists_only_the_sessions_with_a_pane() {
    let store = SqliteStore::open_in_memory().unwrap();
    for id in ["sess-a", "sess-b", "sess-c"] {
        store.register_session(new_session_with(id)).await.unwrap();
    }
    store
        .remember_pane(&SessionId::from("sess-a"), &pane(1))
        .await
        .unwrap();
    store
        .remember_pane(&SessionId::from("sess-c"), &pane(2))
        .await
        .unwrap();

    assert_eq!(
        store.remembered_panes().await.unwrap(),
        vec![
            (SessionId::from("sess-a"), pane(1)),
            (SessionId::from("sess-c"), pane(2)),
        ]
    );
}

/// Remembering a pane for a row that does not exist writes nothing, and asking
/// about it reads `None` rather than failing.
#[tokio::test]
async fn an_unknown_session_remembers_nothing() {
    let store = SqliteStore::open_in_memory().unwrap();
    let missing = SessionId::from("nope");
    store.remember_pane(&missing, &pane(1)).await.unwrap();
    assert_eq!(store.remembered_pane(&missing).await.unwrap(), None);
    store.forget_pane(&missing).await.unwrap();
}

/// A tmux session name belongs to one row at a time: remembering it for one
/// session clears it from any other row still naming it (a record left stale
/// by a pane that died before the name was minted again), and leaves rows
/// naming other panes alone.
#[tokio::test]
async fn remembering_a_name_takes_it_from_any_other_row() {
    let store = SqliteStore::open_in_memory().unwrap();
    for id in ["sess-stale", "sess-new", "sess-other"] {
        store.register_session(new_session_with(id)).await.unwrap();
    }
    let stale = SessionId::from("sess-stale");
    let new = SessionId::from("sess-new");
    let other = SessionId::from("sess-other");
    store.remember_pane(&stale, &pane(1)).await.unwrap();
    store.remember_pane(&other, &pane(2)).await.unwrap();

    store.remember_pane(&new, &pane(1)).await.unwrap();

    assert_eq!(store.remembered_pane(&stale).await.unwrap(), None);
    assert_eq!(store.remembered_pane(&new).await.unwrap(), Some(pane(1)));
    assert_eq!(store.remembered_pane(&other).await.unwrap(), Some(pane(2)));

    // A row that does not exist claims nothing.
    store
        .remember_pane(&SessionId::from("nope"), &pane(1))
        .await
        .unwrap();
    assert_eq!(store.remembered_pane(&new).await.unwrap(), Some(pane(1)));
}
