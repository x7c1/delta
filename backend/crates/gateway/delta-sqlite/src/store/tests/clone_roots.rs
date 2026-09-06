//! Clone-root round trips.

use super::super::SqliteStore;

#[tokio::test]
async fn clone_roots_round_trip_create_list_delete() {
    let store = SqliteStore::open_in_memory().unwrap();

    // A fresh store has no registered clone roots.
    assert!(store.list_clone_roots().await.unwrap().is_empty());

    // Inserts persist verbatim.
    let alpha = store.insert_clone_root("/home/dev/projects").await.unwrap();
    assert_eq!(alpha.path, "/home/dev/projects");
    assert!(!alpha.created_at.is_empty());

    let beta = store
        .insert_clone_root("/work/clones/x7c1")
        .await
        .unwrap();
    assert_eq!(beta.path, "/work/clones/x7c1");

    // Listed both, newest first. The seeded `now_iso8601` may tie at second
    // resolution; the secondary sort key (path ASC) keeps the order stable.
    let listed = store.list_clone_roots().await.unwrap();
    assert_eq!(listed.len(), 2);
    let paths: Vec<&str> = listed.iter().map(|r| r.path.as_str()).collect();
    assert!(paths.contains(&"/home/dev/projects"));
    assert!(paths.contains(&"/work/clones/x7c1"));

    // Inserting a duplicate path is a typed conflict, not a silent overwrite —
    // the PRIMARY KEY constraint is the conflict gate.
    let dup = store.insert_clone_root("/home/dev/projects").await;
    assert!(
        matches!(
            dup,
            Err(delta_usecase::Error::CloneRootDuplicate(ref p)) if p == "/home/dev/projects",
        ),
        "duplicate insert reports `CloneRootDuplicate`, got {dup:?}",
    );

    // Delete removes one row without touching the other.
    store.delete_clone_root("/home/dev/projects").await.unwrap();
    let remaining = store.list_clone_roots().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].path, "/work/clones/x7c1");

    // Deleting an unknown path is a silent no-op (idempotent), not an error.
    store.delete_clone_root("/does/not/exist").await.unwrap();
    assert_eq!(
        store.list_clone_roots().await.unwrap().len(),
        1,
        "unknown delete left the other row untouched"
    );
}
